//! ★★ **The pooled work-stack discipline, extracted once so the four codecs can share it.**
//!
//! # Why this exists
//!
//! `bincode_encoder` reaches **zero allocations and zero bytes per encode** in the steady state,
//! measured and held after an 18,800,104-byte encode. It does that with a thread-local pool:
//! borrow the op-stack allocation, use it, clear it, park it again.
//!
//! `protobuf_encoder` **did not copy that**. It allocates three fresh `Vec`s per encoder — `ops`,
//! `frames`, and a 256-entry `lens` — and that omission **is** the 3.79× shallow regression
//! (~96 → ~363 ns at depth 1, crossover at depth 8, 233.9× faster at 1024). The discipline
//! was written once, in one file, and the next codec did not find it.
//!
//! ⇒ It is written here instead, with the soundness argument stated once rather than
//! re-derived per site — because the argument is the part that is easy to get wrong.
//!
//! # What is shared and what is NOT — the owner's ruling
//!
//! > *"Share logic where it is sensible, use helpers, traits, etc. Whatever is cleanest
//! > architecturally. **Allow specialization among drivers.**"*
//!
//! ★ That is an **AND**, not a choice between one driver and four. What is shared is this
//! pooling discipline and its safety argument. What stays specialized is the **walk**, and
//! the arithmetic refusing to merge those is recorded at [`crate::rust::rholang::drive`]:
//! a `Node` + `Kont` split needs two tags at one offset, which cannot overlay, so a shared
//! `Step` is predicted at **40 B = 5 words against a pinned 4-word ceiling** — derived
//! independently by both codecs, and +64 KiB at depth 4,096.
//!
//! # Why a macro and not a generic type
//!
//! The pooled element is **lifetime-parameterized** — `Op<'a>` borrows the term being
//! encoded — and a `thread_local!` cannot be generic over a caller's lifetime. A
//! `PooledStack<T>` would therefore have to be instantiated at `T = Op<'static>` and
//! transmuted at every use, which puts the `unsafe` back at each call site: exactly what this
//! module exists to prevent.
//!
//! ⇒ [`pooled_stack!`] declares one pool per element type, so the `unsafe` appears **once per
//! type, inside the macro**, under the argument below.
//!
//! # Soundness, stated once
//!
//! The parked vector is **always empty** — [`pooled_stack!`] clears before parking on every
//! path, including a panic, because the owning machine's `Drop` returns the buffer. So the
//! transmute re-types **zero live values**; only the heap allocation crosses, which is the
//! standard buffer-recycling idiom.
//!
//! `T<'a>` and `T<'static>` are **layout-identical**: lifetimes are erased before codegen and
//! appear in no discriminant, size or alignment.
//!
//! Two further properties, each load-bearing:
//!
//! * **Re-entrancy.** If the slot is already borrowed — a nested encode, which the spliced
//!   event-hash emitter really does produce — a private vector is used instead of aliasing.
//!   ⚠ This is not a nicety: aliasing the parked buffer across a nested encode would hand two
//!   live machines the same allocation.
//! * **Convergence.** `give` keeps the **larger** of the parked and returned capacities, so
//!   the pool converges upward to the working set rather than oscillating, and refuses to park
//!   anything past `MAX` so one deep term cannot pin memory.

/// Declare a thread-local pooled stack for one lifetime-parameterized element type.
///
/// ```ignore
/// pooled_stack! {
///     /// The pooled op-stack allocation for the bincode encoder.
///     pool OPS for Op<'_>, capacity = OP_STACK_CAPACITY, max = MAX_POOLED_OPS,
///     take = take_ops, give = give_ops,
/// }
/// ```
///
/// ⚠ The element type is written **without** its lifetime (`Op`, not `Op<'a>`); the macro
/// supplies both `'static` for the parked form and the caller's lifetime for the borrowed one.
#[macro_export]
macro_rules! pooled_stack {
    (
        $(#[$meta:meta])*
        pool $slot:ident for $elem:ident, capacity = $cap:expr, max = $max:expr,
        take = $take:ident, give = $give:ident $(,)?
    ) => {
        // ⚠ The parked type is `'static` because a thread-local cannot be generic over a
        // caller's lifetime. It is ALWAYS EMPTY WHILE PARKED, so no `'static` value of the
        // element type ever exists — see this module's header for the full argument.
        //
        // ⚠ `$(#[$meta])*` is deliberately NOT forwarded here: a `///` at the call site
        // cannot document an item a macro produces, and attaching it would only earn an
        // `unused_doc_comments` warning. Call sites use `//` comments instead.
        thread_local! {
            static $slot: ::std::cell::RefCell<::std::vec::Vec<$elem<'static>>> =
                ::std::cell::RefCell::new(::std::vec::Vec::with_capacity($cap));
        }

        /// Borrow the pooled allocation with the caller's lifetime.
        ///
        /// Falls back to a private vector when the slot is already borrowed, so a **nested**
        /// encode cannot alias the parked buffer.
        fn $take<'a>() -> ::std::vec::Vec<$elem<'a>> {
            $slot.with(|cell| match cell.try_borrow_mut() {
                Ok(mut parked) if parked.is_empty() && parked.capacity() > 0 => {
                    let recycled = ::std::mem::take(&mut *parked);
                    debug_assert!(
                        recycled.is_empty(),
                        concat!(
                            "the pooled stack `",
                            stringify!($slot),
                            "` must be parked empty"
                        )
                    );
                    // SAFETY: `recycled` is empty (asserted), so this re-types zero live
                    // values — only the allocation crosses. `$elem<'a>` and `$elem<'static>`
                    // are layout-identical because lifetimes are erased before codegen.
                    unsafe {
                        ::std::mem::transmute::<
                            ::std::vec::Vec<$elem<'static>>,
                            ::std::vec::Vec<$elem<'a>>,
                        >(recycled)
                    }
                }
                _ => ::std::vec::Vec::with_capacity($cap),
            })
        }

        /// Return an allocation to the pool.
        ///
        /// Parks only if the slot is vacant and the capacity is worth keeping, so neither a
        /// nested encode nor a one-off deep term can pin memory.
        fn $give(mut stack: ::std::vec::Vec<$elem<'_>>) {
            stack.clear();
            if stack.capacity() == 0 || stack.capacity() > $max {
                return;
            }
            // SAFETY: emptied immediately above; see `$take`.
            let parked: ::std::vec::Vec<$elem<'static>> = unsafe { ::std::mem::transmute(stack) };
            $slot.with(|cell| {
                if let Ok(mut slot) = cell.try_borrow_mut() {
                    // Keep the LARGER of the two, so the pool converges upward to the working
                    // set instead of oscillating.
                    if slot.capacity() < parked.capacity() {
                        *slot = parked;
                    }
                }
            });
        }
    };
}
