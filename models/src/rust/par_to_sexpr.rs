use std::fmt::Write as _;

use crate::rhoapi::{Bundle, Expr, New, Par, Receive, Send};
use crate::rust::par_bag::ParBagComponent;
use crate::rust::rholang::protobuf_cursor::{
    CanonicalByteRange as Span, CanonicalField, CanonicalFieldCursor,
};

#[derive(Clone, Copy)]
enum Kind {
    Par,
    Expr,
    Send,
    Receive,
    Bind,
    New,
    Bundle,
    Pair,
    Var,
}

enum Job {
    Message(Kind, Span),
    Repeated { kind: Kind, span: Span, first: bool },
    Text(&'static str),
    Raw(Span),
    Binders { next: i32, end: i32 },
}

fn fields(root: &[u8], span: Span) -> CanonicalFieldCursor<'_> {
    CanonicalFieldCursor::in_prost_slot_order(
        span.slice(root)
            .expect("resident formatter range stays inside its endpoint"),
    )
}

fn next_field<'a>(cursor: &mut CanonicalFieldCursor<'a>) -> Option<CanonicalField<'a>> {
    cursor
        .next_field()
        .expect("resident component has canonical protobuf framing")
}

fn field(root: &[u8], span: Span, tag: u32) -> Option<CanonicalField<'_>> {
    let mut cursor = fields(root, span);
    while let Some(field) = next_field(&mut cursor) {
        if field.number == tag {
            return Some(field);
        }
    }
    None
}

fn child(parent: Span, field: CanonicalField<'_>) -> Span {
    field
        .expect_length_delimited()
        .expect("resident child is length delimited");
    Span {
        start: parent.start + field.payload_range.start,
        end: parent.start + field.payload_range.end,
    }
}

fn child_field(root: &[u8], parent: Span, tag: u32) -> Span {
    field(root, parent, tag).map_or(Span::default(), |field| child(parent, field))
}

fn run(root: &[u8], parent: Span, tag: u32) -> Span {
    let mut cursor = fields(root, parent);
    let mut span = Span::default();
    let mut found = false;
    while let Some(field) = next_field(&mut cursor) {
        if field.number == tag {
            if !found {
                span.start = parent.start + field.encoded_range.start;
            }
            span.end = parent.start + field.encoded_range.end;
            found = true;
        } else if found {
            break;
        }
    }
    span
}

fn signed(value: u64) -> i64 { ((value >> 1) as i64) ^ -((value & 1) as i64) }

fn classify(counts: impl IntoIterator<Item = u64>) -> u8 {
    counts
        .into_iter()
        .fold(0, |total, count| (total + count.min(2) as u8).min(2))
}

#[derive(Default)]
struct Renderer {
    out: String,
    work: Vec<Job>,
    #[cfg(test)]
    maximum_pending: usize,
    #[cfg(test)]
    endpoints: usize,
}

impl Renderer {
    fn repeated(&mut self, kind: Kind, span: Span) {
        if !span.is_empty() {
            self.work.push(Job::Repeated {
                kind,
                span,
                first: true,
            });
        }
    }

    fn unary(&mut self, root: &[u8], span: Span, open: &'static str, tag: u32) {
        self.out.push_str(open);
        self.work.push(Job::Text(")"));
        self.work
            .push(Job::Message(Kind::Par, child_field(root, span, tag)));
    }

    fn binary(&mut self, root: &[u8], span: Span, open: &'static str, separator: &'static str) {
        self.out.push_str(open);
        self.work.push(Job::Text(")"));
        self.work
            .push(Job::Message(Kind::Par, child_field(root, span, 2)));
        self.work.push(Job::Text(separator));
        self.work
            .push(Job::Message(Kind::Par, child_field(root, span, 1)));
    }

    fn par(&mut self, root: &[u8], span: Span) {
        let mut runs = [Span::default(); 5];
        let mut present = [false; 5];
        let mut count = 0u8;
        let mut cursor = fields(root, span);
        while let Some(field) = next_field(&mut cursor) {
            let index = match field.number {
                1 => 0,
                2 => 1,
                4 => 2,
                5 => 3,
                11 => 4,
                _ => continue,
            };
            if !present[index] {
                runs[index].start = span.start + field.encoded_range.start;
                present[index] = true;
            }
            runs[index].end = span.start + field.encoded_range.end;
            count = (count + 1).min(2);
        }
        if count == 0 {
            self.out.push_str("Nil");
            return;
        }
        if count == 2 {
            self.out.push_str("(par ");
            self.work.push(Job::Text(")"));
        }
        let kinds = [
            Kind::Send,
            Kind::Receive,
            Kind::New,
            Kind::Expr,
            Kind::Bundle,
        ];
        for index in (0..5).rev() {
            if !present[index] {
                continue;
            }
            self.repeated(kinds[index], runs[index]);
            if present[..index].iter().any(|value| *value) {
                self.work.push(Job::Text(" "));
            }
        }
    }

    fn expr(&mut self, root: &[u8], span: Span) {
        let Some(value) = next_field(&mut fields(root, span)) else {
            self.out.push_str("Nil");
            return;
        };
        match value.number {
            1 => write!(
                self.out,
                "{}",
                value.expect_varint().expect("resident bool") != 0
            )
            .expect("String writes are infallible"),
            2 => write!(
                self.out,
                "{}",
                signed(value.expect_varint().expect("resident sint64"))
            )
            .expect("String writes are infallible"),
            3 | 4 => {
                let delimiter = if value.number == 3 { '"' } else { '`' };
                self.out.push(delimiter);
                self.out.push_str(
                    std::str::from_utf8(
                        value
                            .expect_length_delimited()
                            .expect("resident text bytes"),
                    )
                    .expect("resident text is UTF-8"),
                );
                self.out.push(delimiter);
            }
            25 => {
                self.out.push_str("0x");
                for byte in value
                    .expect_length_delimited()
                    .expect("resident byte array")
                {
                    const HEX: &[u8; 16] = b"0123456789abcdef";
                    self.out.push(HEX[(byte >> 4) as usize] as char);
                    self.out.push(HEX[(byte & 15) as usize] as char);
                }
            }
            5 | 6 => self.unary(
                root,
                child(span, value),
                if value.number == 5 { "(not " } else { "(- " },
                1,
            ),
            7..=10 => {
                let open = match value.number {
                    7 => "(* ",
                    8 => "(/ ",
                    9 => "(+ ",
                    _ => "(- ",
                };
                self.binary(root, child(span, value), open, " ");
            }
            19 => {
                self.work.push(Job::Message(
                    Kind::Var,
                    child_field(root, child(span, value), 1),
                ));
            }
            20..=23 => {
                let (open, close, kind) = match value.number {
                    20 => ("[", "]", Kind::Par),
                    21 => ("(tuple ", ")", Kind::Par),
                    22 => ("(set ", ")", Kind::Par),
                    _ => ("(map ", ")", Kind::Pair),
                };
                self.out.push_str(open);
                self.work.push(Job::Text(close));
                self.repeated(kind, run(root, child(span, value), 1));
            }
            24 => {
                let message = child(span, value);
                self.out.push('(');
                self.work.push(Job::Text(")"));
                self.repeated(Kind::Par, run(root, message, 3));
                self.work.push(Job::Text(" "));
                self.work.push(Job::Raw(child_field(root, message, 1)));
                self.work.push(Job::Text("."));
                self.work
                    .push(Job::Message(Kind::Par, child_field(root, message, 2)));
            }
            _ => self.out.push_str("(expr)"),
        }
    }

    fn render(&mut self, root: &[u8], kind: Kind) {
        #[cfg(test)]
        {
            self.endpoints += 1;
        }
        assert!(
            self.work.is_empty(),
            "previous endpoint traversal must be complete"
        );
        self.work.push(Job::Message(kind, Span {
            start: 0,
            end: root.len(),
        }));
        while let Some(job) = self.work.pop() {
            #[cfg(test)]
            {
                self.maximum_pending = self.maximum_pending.max(self.work.len() + 1);
            }
            match job {
                Job::Text(text) => self.out.push_str(text),
                Job::Raw(span) => self.out.push_str(
                    std::str::from_utf8(span.slice(root).expect("resident text range"))
                        .expect("resident text is UTF-8"),
                ),
                Job::Binders { next, end } => {
                    if next < end {
                        if next != 0 {
                            self.out.push(' ');
                        }
                        write!(self.out, "x{next}").expect("String writes are infallible");
                        self.work.push(Job::Binders {
                            next: next + 1,
                            end,
                        });
                    }
                }
                Job::Repeated { kind, span, first } => {
                    let Some(field) = next_field(&mut fields(root, span)) else {
                        continue;
                    };
                    if !first {
                        self.out.push(' ');
                    }
                    let remainder = Span {
                        start: span.start + field.encoded_range.end,
                        end: span.end,
                    };
                    if !remainder.is_empty() {
                        self.work.push(Job::Repeated {
                            kind,
                            span: remainder,
                            first: false,
                        });
                    }
                    self.work.push(Job::Message(kind, child(span, field)));
                }
                Job::Message(kind, span) => match kind {
                    Kind::Par => self.par(root, span),
                    Kind::Expr => self.expr(root, span),
                    Kind::Send => {
                        self.out.push_str("(! ");
                        self.work.push(Job::Text(")"));
                        self.repeated(Kind::Par, run(root, span, 2));
                        self.work.push(Job::Text(" "));
                        self.work
                            .push(Job::Message(Kind::Par, child_field(root, span, 1)));
                    }
                    Kind::Receive => {
                        self.out.push_str("(for (");
                        self.work.push(Job::Text(")"));
                        self.work
                            .push(Job::Message(Kind::Par, child_field(root, span, 2)));
                        self.work.push(Job::Text(") "));
                        self.repeated(Kind::Bind, run(root, span, 1));
                    }
                    Kind::Bind => self.unary(root, span, "(bind <- ", 2),
                    Kind::New => {
                        self.out.push_str("(new ");
                        self.work.push(Job::Text(")"));
                        self.work
                            .push(Job::Message(Kind::Par, child_field(root, span, 2)));
                        self.work.push(Job::Text(" "));
                        let end = field(root, span, 1).map_or(0, |field| {
                            signed(field.expect_varint().expect("resident bind count")) as i32
                        });
                        self.work.push(Job::Binders { next: 0, end });
                    }
                    Kind::Bundle => self.unary(root, span, "(bundle ", 1),
                    Kind::Pair => self.binary(root, span, "(", " : "),
                    Kind::Var => match next_field(&mut fields(root, span)) {
                        None => self.out.push_str("var"),
                        Some(field) => match field.number {
                            1 | 2 => {
                                self.out.push(if field.number == 1 { '_' } else { '$' });
                                write!(
                                    self.out,
                                    "{}",
                                    signed(field.expect_varint().expect("resident variable index"))
                                        as i32
                                )
                                .expect("String writes are infallible");
                            }
                            3 => self.out.push('_'),
                            _ => unreachable!("resident variable has a known oneof arm"),
                        },
                    },
                },
            }
        }
    }

    fn family<T: ParBagComponent>(&mut self, par: &Par, kind: Kind, first: &mut bool) {
        par.visit_encoded::<T>(|entry| {
            if !*first {
                self.out.push(' ');
            }
            *first = false;
            let start = self.out.len();
            self.render(entry.payload(), kind);
            let end = self.out.len();
            for _ in 1..entry.multiplicity().get() {
                self.out.push(' ');
                self.out.extend_from_within(start..end);
            }
        });
    }
}

pub struct ParToSExpr;

impl ParToSExpr {
    pub fn par_to_sexpr(par: &Par) -> String {
        let count = classify([
            par.family_cardinality::<Send>(),
            par.family_cardinality::<Receive>(),
            par.family_cardinality::<New>(),
            par.family_cardinality::<Expr>(),
            par.family_cardinality::<Bundle>(),
        ]);
        let mut renderer = Renderer::default();
        match count {
            0 => {
                return "Nil".to_owned();
            }
            2 => renderer.out.push_str("(par "),
            _ => {}
        }
        let mut first = true;
        renderer.family::<Send>(par, Kind::Send, &mut first);
        renderer.family::<Receive>(par, Kind::Receive, &mut first);
        renderer.family::<New>(par, Kind::New, &mut first);
        renderer.family::<Expr>(par, Kind::Expr, &mut first);
        renderer.family::<Bundle>(par, Kind::Bundle, &mut first);
        if count == 2 {
            renderer.out.push(')');
        }
        renderer.out
    }
}

#[cfg(test)]
#[path = "../../tests/support/par_to_sexpr.rs"]
pub(crate) mod tests;
