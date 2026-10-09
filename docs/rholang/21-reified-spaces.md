# 21. Reified Spaces

A Reified space is a tuple space that a program creates and uses as a value [[FIPS-RRS]](#ref-fips-rrs).
A factory creates the space, and `use` blocks and space annotations route the channels of a program into it.
The [Reified RSpaces specification](../rspace/reified-rspaces/specification.md) states the rules of every space family, and the [syntax contract](../rspace/reified-rspaces/syntax-contract.md) states the syntax.

Phase P6 of the campaign writes the section on VectorDB spaces below.
Phase P10 completes the chapter with the other families and the section "Writing your own space".

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

## References

<a id="ref-fips-rrs"></a>[FIPS-RRS] M. Stay and L. G. Meredith, "Reifying RSpaces," F1r3fly Improvement Proposal, 2025-09-26. [FIPS repository, approved/2025-09-26-Reifying-RSpaces](https://github.com/F1R3FLY-io/FIPS/blob/main/approved/2025-09-26-Reifying-RSpaces/Reifying%20RSpaces.md)
