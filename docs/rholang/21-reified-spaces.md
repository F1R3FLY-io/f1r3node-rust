# 21. Reified Spaces

A Reified space is a tuple space that a program creates and uses as a value [[FIPS-RRS]](#ref-fips-rrs).
A factory creates the space, and `use` blocks and space annotations route the channels of a program into it.
The [Reified RSpaces specification](../rspace/reified-rspaces/specification.md) states the rules of every space family, and the [syntax contract](../rspace/reified-rspaces/syntax-contract.md) states the syntax.

Phase P6 of the campaign writes the section on VectorDB spaces below, and phase P8 writes the section "Writing your own space".
Phase P10 completes the chapter with the other families.

## VectorDB spaces

A VectorDB space stores records that can carry a vector, and a receive takes the records that are most similar to a query vector.
The families are `rho:space:hashmap:vectordb` and `rho:space:vector:vectordb`.
Every decision of the space compares exact numbers, so every node selects the same records ([decision 54](../rspace/reified-rspaces/decision-register.md#decision-54)).

### Creating a space

The factory takes the params `capacity` and `dimensions`, and it can take `metric` and `mirror`.

| Param | Rule |
|---|---|
| `capacity` | Required. The most records of each collection, an `Int` from 1 to 1,000,000. |
| `dimensions` | Required. The length of every vector of the space, an `Int` from 1 to 4,096. |
| `metric` | Optional. `cosine` (the default), `dot`, `euclidean`, `manhattan`, `hamming` or `jaccard`. |
| `mirror` | Optional. `"chroma"` keeps a copy of the records in ChromaDB for advisory queries ([ChromaDB](chromadb.md#chromadb-as-the-search-backend-of-reified-vectordb-spaces)). |

### Storing records and querying them

A send stores its payload as a record.
When the payload is one list of numbers, that list is the vector of the record.
A receive with a query vector takes the most similar record.

```rholang
new vdb(`rho:space:hashmap:vectordb`) in { for (space <- vdb!?("default"; capacity=8, dimensions=2)) { use space { @"docs"!([1, 0]) | @"docs"!([0, 1]) | for (@v <- @"docs" ~ ([1, 0])) { Nil } } } }
```

The query `~ ([1, 0])` selects by the metric of the space, here `cosine`.
If the receive runs after the two sends, it takes the record `[1, 0]`, whose cosine with the query is 1.
If it runs first, it waits, and the first send that is eligible for its query fires it.

A record can also keep another payload and give its vector with the send param `embedding`.
A list query with `k` binds one list: the payload values of the first `k` eligible records, in the order of their scores.

```rholang
new vdb(`rho:space:hashmap:vectordb`) in { for (space <- vdb!?("default"; capacity=8, dimensions=2, metric="euclidean")) { use space { @"docs"!("origin"; embedding=[0, 0]) | @"docs"!("far"; embedding=[3, 4]) | for (@top <- @"docs" ~ ([0, 0]; radius=5, k=2, scores=true)) { Nil } } } }
```

The query keeps the records within the distance 5 of `[0, 0]`, the nearest first.
With `scores=true`, each element of the list is `[score, value]`.
When the receive runs after both sends, `top` is `[[0.0, "origin"], [5.0, "far"]]`.
When it runs earlier, it fires as soon as one record is eligible, with the eligible records of that moment.

### The keys of a query

| Key | Meaning |
|---|---|
| the query vector | The vector of the query. Without it, the receive reads the records in their canonical order. |
| `metric` | The metric of this query, instead of the metric of the space. |
| `threshold` | The least similarity, from 0 to 1, for `cosine`, `dot` and `jaccard`. |
| `radius` | The greatest distance, at least 0, for `euclidean`, `manhattan` and `hamming`. |
| `k` | List mode: the list of the first `k` eligible records. |
| `results` | List mode with `"all"`: the list of all eligible records. |
| `scores` | `true` delivers `[score, value]` for each record. |
| `ids` | Only the records whose ids are in this list, for example the ids that `rho:vectordb:ann` gives. |

`threshold` and `radius` exclude each other, and so do `k` and `results`.
A receive in list mode cannot take part in a join with other channels.
Only a linear receive `<-` takes a query in source text.

### Capacity and merges

A store into a collection that holds `capacity` records fails the deploy with `CapacityExceeded`.
Two parallel branches that store into one collection merge with no conflict.
So a merged collection can hold more than `capacity` records for a time.
While it does, every store fails until removals bring the count below `capacity` ([decision 71](../rspace/reified-rspaces/decision-register.md#decision-71)).

### Vector arithmetic

The system process `rho:lang:vector` adds, scales and multiplies vectors exactly, and it computes norms and distances.
[Chapter 10](10-system-channels.md#reified-rspaces-vectors-and-the-vectordb-services) describes it, with the processes `rho:vectordb:embed` and `rho:vectordb:ann` of the ChromaDB mirror.

## Writing your own space

A program can implement a space itself [[FIPS-RRS]](#ref-fips-rrs).
Any agent that answers three methods can serve as a space.
A program names it as it names a native space: in a `use` block or in an annotation `new x : a`.
The node translates the names, sends and receives of such a space into method calls on the agent ([agent API](../rspace/reified-rspaces/agent-api.md)).

### The methods of a space agent

| Method | When the node calls it | What the agent does |
|---|---|---|
| `gensym()` | A `new` binds a name of the space. | It replies one fresh name, and the node binds the name of the agent with it. |
| `produce(raw, msg)` | A program sends on a name of the space. `raw` is the channel, and `msg` is the list of the data. | It stores the message, or it fires a stored consume. |
| `consume(pairs, arrow, body)` | A program receives on names of the space. `pairs` holds one pair `(matcher, raw)` for each bind. | It fires on stored messages, or it stores the consume. |

The node calls each method in the call shape of the Agents FIPS [[FIPS-AGENTS]](#ref-fips-agents): the return channel, the method name, and then the arguments.
The return channel of `produce` and `consume` is a sink that absorbs any reply.
A matcher `m` answers the call `m!(ret, msg)` with `(true, env)` when `msg` matches its bind and passes the guard of the bind, and with `(false, Nil)` otherwise.
The map `env` holds the values of the binders of the bind.
The body capability takes one map: the union of the maps `env` of all binds.
The arrow is `"<-"` for a linear receive, `"<<-"` for a peek and `"<="` for a persistent receive.

### The obligations of an agent

The node does not check the behavior of an agent.
The author of an agent keeps three obligations:

- **ASO-1.** After each method completes, no stored consume and stored messages exist such that every matcher of the consume accepts its message.
- **ASO-2.** For the arrows `"<-"` and `"<<-"`, the agent sends to the body capability at most once. `"<-"` removes the chosen messages, and `"<<-"` keeps them.
- **ASO-3.** The agent replies to each `gensym` call exactly once, with a fresh name.

An agent that breaks an obligation breaks the semantics of its space, but it never breaks consensus.
Each call is an ordinary event of the default space, so every node charges it and replays it in the same way.
For example, an agent that never answers leaves the receives of its space pending, and replay reproduces the pending receives.

### The reference agent

The example `examples/reified_rspaces/agent_hashmap_bag.rho` is an agent of the Agents FIPS.
Each instance is a space with HashMap channels and bag collections, and the constructor takes the qualifier `"default"`.

```rholang
// The reference user agent of Reified RSpaces (P8, design v2 W8).
//
// agentHashmapBag is an agent of the Agents FIPS. Each instance is a space with HashMap
// channels and bag collections. A program names the space in a `use` block or in an annotation
// `new x : space`, and the node translates the names, sends and receives of the space into
// three method calls on the instance:
//
//   gensym()                     reply one fresh name
//   produce(raw, msg)            the send of the data list msg on the channel raw
//   consume(pairs, arrow, body)  a receive: one (matcher, raw) pair for each bind, the arrow
//                                "<-", "<<-" or "<=", and the body capability
//
// A matcher m answers m!(ret, msg) with (true, env) when msg matches its bind and passes its
// guard, and with (false, Nil) otherwise. The body capability takes the union of the env maps
// of the binds. The return channel of produce and consume is a sink that absorbs any reply.
//
// An instance keeps its state on one channel, so its methods run one at a time. The state is a
// map from each channel to its stored messages, in insertion order, and the list of the stored
// consumes, in insertion order. A produce offers the new message to the stored consumes in
// order, and a consume tries the stored messages at once. A peek keeps the message for the next
// stored consume, and a persistent consume fires until no stored messages satisfy it. So no
// stored consume and stored messages match when a method completes (ASO-1). A linear consume
// runs its body at most once (ASO-2), and gensym replies exactly once (ASO-3).
//
// The instance waits for the answer of each matcher while it holds its state. So a caller that
// brings a matcher that never answers stops the instance. Give the reference of a space only to
// programs that you trust.
new agentHashmapBag, offer, drain, find in {
  // offer: the stored messages hold a new message. From position k on, fire the first stored
  // consume that the stored messages satisfy. A fire takes the new message, because no stored
  // consume matched before it arrived. A peek keeps the message, so the next consumes get it.
  contract offer(@instance, @messages, @consumes, @k) = {
    if (k == consumes.length()) {
      @(instance, "state")!(messages, consumes)
    } else {
      match consumes.nth(k) {
        (pairs, arrow, body) => {
          new found in {
            find!(pairs, 0, 0, messages, {}, *found) |
            for (@result <- found) {
              match result {
                (true, left, env) => {
                  @body!(env) |
                  match [arrow, consumes.slice(0, k) ++ consumes.slice(k + 1, consumes.length())] {
                    ["<<-", others] => {
                      offer!(instance, messages, others, k)
                    }
                    ["<-", others] => {
                      @(instance, "state")!(left, others)
                    }
                    _ => {
                      @(instance, "state")!(left, consumes)
                    }
                  }
                }
                _ => {
                  offer!(instance, messages, consumes, k + 1)
                }
              }
            }
          }
        }
      }
    }
  } |

  // drain: fire a new consume on the stored messages, or store it. A persistent consume fires
  // again until the stored messages no longer satisfy it, and then it is stored.
  contract drain(@instance, @messages, @consumes, @pairs, @arrow, @body) = {
    new found in {
      find!(pairs, 0, 0, messages, {}, *found) |
      for (@result <- found) {
        match result {
          (true, left, env) => {
            @body!(env) |
            match arrow {
              "<<-" => {
                @(instance, "state")!(messages, consumes)
              }
              "<-" => {
                @(instance, "state")!(left, consumes)
              }
              _ => {
                drain!(instance, left, consumes, pairs, arrow, body)
              }
            }
          }
          _ => {
            @(instance, "state")!(messages, consumes ++ [(pairs, arrow, body)])
          }
        }
      }
    }
  } |

  // find: the first choice, in insertion order, of distinct stored messages for the binds of
  // pairs from bind i on, such that the matcher of each bind accepts its message. Bind i tries
  // its stored messages from position j on. A message that the matcher accepts leaves the
  // stored messages for the later binds, so a join that names one channel twice takes two
  // distinct messages. The answer is (true, left, env), with the stored messages that remain
  // and the union of the env maps, or (false, Nil, Nil).
  contract find(@pairs, @i, @j, @messages, @env, ret) = {
    if (i == pairs.length()) {
      ret!((true, messages, env))
    } else {
      match pairs.nth(i) {
        (matcher, channel) => {
          match messages.getOrElse(channel, []) {
            stored => {
              if (j == stored.length()) {
                ret!((false, Nil, Nil))
              } else {
                new answer, deeper in {
                  @matcher!(*answer, stored.nth(j)) |
                  for (@reply <- answer) {
                    match reply {
                      (true, bound) => {
                        match stored.slice(0, j) ++ stored.slice(j + 1, stored.length()) {
                          [] => {
                            find!(pairs, i + 1, 0, messages.delete(channel), env.union(bound), *deeper)
                          }
                          rest => {
                            find!(pairs, i + 1, 0, messages.set(channel, rest), env.union(bound), *deeper)
                          }
                        } |
                        for (@result <- deeper) {
                          match result {
                            (true, _, _) => {
                              ret!(result)
                            }
                            _ => {
                              find!(pairs, i, j + 1, messages, env, *ret)
                            }
                          }
                        }
                      }
                      _ => {
                        find!(pairs, i, j + 1, messages, env, *ret)
                      }
                    }
                  }
                }
              }
            }
          }
        }
        _ => {
          ret!((false, Nil, Nil))
        }
      }
    }
  } |

  agent agentHashmapBag {
    constructor(@"default") {
      @(*private, "state")!({}, [])
    } |
    method gensym() {
      new raw in { return!(*raw) }
    } |
    method produce(@raw, @msg) {
      for (@messages, @consumes <- @(*private, "state")) {
        offer!(*private, messages.set(raw, messages.getOrElse(raw, []) ++ [msg]), consumes, 0)
      }
    } |
    method consume(@pairs, @arrow, @body) {
      match [pairs, arrow] {
        [[_, ..._], "<-" \/ "<<-" \/ "<="] => {
          for (@messages, @consumes <- @(*private, "state")) {
            drain!(*private, messages, consumes, pairs, arrow, body)
          }
        }
        _ => {
          Nil
        }
      }
    } |
    default(...@args) {
      Nil
    }
  } |

  // Demo: a space of the agent in a use block. A send and a receive on a name of the space meet
  // through the agent, and the sum of the received values reaches a public channel.
  new out in {
    for (space <- agentHashmapBag!?("default")) {
      use space {
        new x in {
          x!(1, 2) |
          for (@a, @b <- x) { out!(a + b) }
        }
      }
    } |
    for (@sum <- out) { @"agentHashmapBag.demo"!(sum) }
  }
}
```

An instance keeps its state on one channel, so the methods of one instance run one at a time.
The state maps each channel to its stored messages, and it lists the stored consumes, both in insertion order.

A `produce` stores the message, and the contract `offer` gives it to the stored consumes in insertion order.
A `consume` runs the contract `drain`, which fires at once on the stored messages or stores the consume.
The contract `find` tries the stored messages of each bind in insertion order and calls one matcher at a time.
A message that a matcher accepts leaves the candidates of the later binds, so a join that names one channel twice takes two distinct messages.
A fire keeps the chosen messages only for `"<<-"`, and it keeps the consume only for `"<="` (ASO-2).
A persistent consume fires again until the stored messages no longer satisfy it, so no match stays pending (ASO-1).

### Cost

Each operation on an agent space runs ordinary COMMs in the default space.
A send makes one call of the agent.
A receive makes one call, and it installs one matcher receive for each bind and one body receive.
Each matcher call costs three COMMs: the call, the forwarder hop to the matcher process, and the reply.
So an operation on an agent space costs more than the same operation on a native space, whose handle runs the match itself.

### Trust

An agent sends the values of a receive to the body capability.
So a program trusts the agent to send the values that the matchers gave.
A native space accepts only the matchers that the node built, but an agent calls the matchers that a consume brings.
The reference agent waits for the answer of each matcher while it holds its state, so a matcher that never answers stops the instance.
Give the reference of an agent space only to programs that you trust.
The call `getSpaceAgent` on an agent space replies `NotANativeSpace`, because the node does not know the constructor of an agent.

### The conformance kit

The test suite `rholang/tests/reified_agent_conformance.rs` checks an agent through programs that use the agent as a space.
The kit runs each case as a sequence of deploys, and it compares all outputs after each deploy.
So a match that stays pending fails the case (ASO-1).
The cases cover `gensym`, the three arrows, a join, a guard, a join over one channel twice, and two waiting receives.

The kit runs each case against the reference agent and against a wrapper agent that forwards each call to a native `rho:space:hashmap:bag` space.
The negative control is a faulty agent whose `consume` stores each receive without a try.
The kit fails at the step that leaves its match pending.
Another agent joins the kit with one value `AgentUnderTest`, which holds the deploy that creates the agent.

## References

<a id="ref-fips-rrs"></a>[FIPS-RRS] M. Stay and L. G. Meredith, "Reifying RSpaces," F1r3fly Improvement Proposal, 2025-09-26. [FIPS repository, approved/2025-09-26-Reifying-RSpaces](https://github.com/F1R3FLY-io/FIPS/blob/main/approved/2025-09-26-Reifying-RSpaces/Reifying%20RSpaces.md)

<a id="ref-fips-agents"></a>[FIPS-AGENTS] M. Stay, "Agents," F1r3fly Improvement Proposal, 2025-08-20. [FIPS repository, approved/2025-08-20-Agents](https://github.com/F1R3FLY-io/FIPS/blob/main/approved/2025-08-20-Agents/Agents.md)
