# ChromaDB Integration for F1r3fly
In this document, we detail the ChromaDB integration for F1r3fly. We will describe how it is intended to be used in smart contracts, as well as describing the key channels that are added by this project.

## Intended Use
Retrieval-Augmented Generation (RAG) is a technique that allows an LLM to access information from sources that it was not trained on. An important piece of RAG technology is the database that can identify relevant documents and supply them for augmentation in the LLM pipeline. SingularityNet, a partner of F1r3fly, wishes to be able to use RAG with F1r3fly.

## Using Chroma With F1r3fly
By default, F1r3fly will not build with ChromaDB support. This is because ChromaDB has a lot of dependencies that need to be pulled during compilation. In situations where ChromaDB support is not needed, this can cause compile times to be longer than necessary.

To compile F1r3fly with ChromaDB support, you can use cargo's `feature` flag: `cargo run --package rholang --release --features "chromadb"`. In order to use ChromaDB, you must have a running ChromaDB instance. You can start one by invoking `docker compose -f docker/shard.yml --profile chromadb up`.

Additionally, when starting F1r3fly for the first time with ChromaDB support, the ONNX representation of `sentence-transformers/all-MiniLM-L6-v2` and its tokenizer are downloaded from Hugging Face over Rustls and cached locally. Subsequent starts load the model from that cache and do not require network access. Set `FASTEMBED_CACHE_DIR` to place the cache on durable storage; otherwise it is stored in `.fastembed_cache` relative to the node working directory. `HF_HOME` takes precedence when it is set, and `HF_ENDPOINT` selects an approved Hugging Face mirror.

If, for whatever reason, the F1r3fly node is unable to interact with the ChromaDB service, a log message is recorded with `info!` and all subsequent `rho:chroma:*` calls will behave as NoOps. 

### Environment Variables
The `chroma` crate uses environment variables to identify how to communicate with ChromaDB. The following table lists the environment variables along with their default values if the environment variable cannot be found.

| Variable          | Default                 |
| -------------------| -------------------------|
| `CHROMA_ENDPOINT` | `http://localhost:8000` |
| `CHROMA_TENANT`   | `"default_tenant"`      |
| `CHROMA_DATABASE` | `"default_database"`    |
| `FASTEMBED_CACHE_DIR` | `.fastembed_cache` |
| `HF_HOME` | unset |
| `HF_ENDPOINT` | Hugging Face public endpoint |


## Design Assumptions and Consensus Semantics
This section clarifies how ChromaDB integrates with F1r3fly’s execution, validation, and replay model. The following reflects the assumptions under which the current design operates.

### Consensus vs Local State
ChromaDB is not treated as canonical consensus state. Instead, the canonical state is the set of Rholang operations (e.g., collection creation, document insertion, deletion) recorded on-chain. ChromaDB acts as a derived, local materialized index built from this canonical state.

Implications:
- Nodes are not required to persist ChromaDB state across restarts.
- Nodes must be able to reconstruct equivalent ChromaDB state from replay.

### Execution and Block Validation
ChromaDB operations (e.g., insert, delete) are executed as part of deploy processing, but block validity does not depend on ChromaDB internals, and validation is based solely on deterministic Rholang state transitions. ChromaDB is therefore treated as an auxiliary system that reflects on-chain data and does not define consensus.

If a node’s local ChromaDB state is missing or corrupted, then the node can still validate blocks and is expected to rebuild the index via replay.

### Replay Semantics
During replay, all Chroma-related operations (collection creation, document insertion, deletion) are re-executed in block order. This reconstructs the ChromaDB index from canonical on-chain data.

This ensures consistency of indexed data across nodes and no reliance on persisted off-chain state.

### Query Semantics and Determinism
Query operations (e.g., `rho:chroma:collection:entries:query`) return results based on similarity search.

Assumptions:
- Query results are not consensus-critical
- They do not affect block validation or state transitions

The rationale here is that similarity search may depend on factors such as embedding generation, floating-point behavior, and index structure. Further, these may not be strictly deterministic across all nodes. Therefore, query results are treated as advisory or application-level outputs, and smart contracts must not rely on query results for consensus-critical behavior.

### Non-Determinism Considerations
Because query operations are not guaranteed to be deterministic across nodes, query operations should be treated as non-deterministic, and therefore inclusion in `non_deterministic_ops()` is appropriate if they are exposed within consensus execution.

Mutation operations (such as insert and delete) are deterministic, as they operate on explicit inputs recorded on-chain and therefore do not require inclusion in `non_deterministic_ops()`.

### Open Questions / Areas for Clarification
The following aspects may require confirmation from system designers:
- Whether query results should ever influence consensus behavior
- Whether ChromaDB should instead be treated as consensus-critical state

If any of these assumptions are incorrect, the design and implementation must be adjusted accordingly.

## Added Channels
In this section, we specify all channels that have been added to Rholang by this project.

### `rho:chroma:collection:new`
This channel creates a new ChromaDB collection. Its arguments are as follows:
- `collection_name`: A string representing the name of the collection to be created
- `ignore_if_exists`: A boolean. If true, then if the collection already exists, no action will be taken. If false, the collection's metadata will be updated using the data from the next argument
- `metadata`: A dictionary adhering to any schema. Represents additional metadata to be stored with the collection
- `ack`: The recipient of the output

This channel's output is empty.

#### Example
```
new createCollection(`rho:chroma:collection:new`), stdout(`rho:io:stdout`), retCh in {
  createCollection!("foo", true, {"meta1" : 1, "two" : "42", "three" : 42, "meta2": "bar"}, *retCh) |
  for(@res <- retCh) {
      stdout!(res)
  }
}
```

### `rho:chroma:collection:meta`
This channel fetches a given collection's metadata. Its arguments are as follows:
- `collection_name`: A string representing the name of the collection whose metadata we want
- `ack`: The recipient of the output

This channel's output is a dictionary containing the collection's metadata.

#### Example
```
new getCollectionMeta(`rho:chroma:collection:meta`), stdout(`rho:io:stdout`), retCh in {
  getCollectionMeta!("foo", *retCh) |
  for(@res <- retCh) {
      stdout!(res)
  }
}
```

### `rho:chroma:collection:entries:new`
This channel adds documents to a collection. Its arguments are as follows:
- `collection_name`: A string representing the name of the collection to be added to
- `entries`: A dictionary where the keys are document IDs and the values are a tuple with two entries: the text of the document itself and a dictionary representing metadata
- `ack`: The recipient of the output

This channel's output is a string containing the collection name.

#### Example
```
new upsertEntries(`rho:chroma:collection:entries:new`), stdout(`rho:io:stdout`), retCh in {
  upsertEntries!(
    "foo",
    { "doc1": ("Hello world!", Nil),
      "doc2": (
        "Hello world again!",
        { "meta1": "42" }
      )
    },
    *retCh
  ) |
  for(@res <- retCh) {
      stdout!(res)
  }
}
```

### `rho:chroma:collection:entries:query`
This channel finds the 10 documents that are most similar to the input query. If fewer than 10 documents exist, it will return all documents. Documents are returned in order of most relevant to least relevant. Its arguments are as follows:
- `collection_name`: A string representing the name of the collection to be queried
- `doc_texts`: A list of strings to be queried
- `ack`: The recipient of the output

This channel's output is a list of document entries.

#### Example
```
new queryEntries(`rho:chroma:collection:entries:query`), stdout(`rho:io:stdout`), retCh in {
  queryEntries!("foo", [ "Hello world" ], *retCh) |
  for(@res <- retCh) {
      stdout!(res)
  }
}
```

### `rho:chroma:collection:entries:delete`
This channel deletes the given entries from the given collection. Its arguments are as follows:
- `collection_name`: A string representing the name of the collection to be queried
- `doc_ids`: A list of strings representing the IDs of the documents to be deleted
- `ack`: The recipient of the output

This channel's output is the name of the collection.

#### Example
```
new deleteEntries(`rho:chroma:collection:entries:delete`), stdout(`rho:io:stdout`), retCh in {
  deleteEntries!("foo", [ "doc1" ], *retCh) |
  for(@res <- retCh) {
      stdout!(res)
  }
}
```

## ChromaDB as the search backend of Reified VectorDB spaces

Phase P6 of Reified RSpaces adds the VectorDB spaces `rho:space:hashmap:vectordb` and `rho:space:vector:vectordb`.
A VectorDB space can keep a copy of its records in ChromaDB, its mirror, and answer advisory queries from it.
This section explains how the mirror works and how to run a node with it.
The rules are in the [Reified RSpaces specification](../rspace/reified-rspaces/specification.md#10-the-chromadb-mirror), and the [design](../rspace/reified-rspaces/design.md#n16-phase-p6-the-chromadb-search-backend) explains the code.

The mirror differs from the `rho:chroma:*` channels above in one main point: it never decides a consensus result.
The exact engine of the space selects every record that a receive takes, and the mirror only gives hints.
So a node with a stale mirror, or with no mirror, computes the same state as every other node.

### The three layers

A VectorDB space has three layers:

1. The frontend: the factory, the sends with the param `embedding`, and the receives with similarity queries.
2. RSpace: the store of the consensus records, with the state root, replay, merges and LFS.
3. ChromaDB: an optional mirror of the records on each node, which answers the advisory query `rho:vectordb:ann`.

A space has a mirror when its factory call names `mirror = "chroma"`.
The space must have the default qualifier and a metric that ChromaDB indexes: `cosine`, `dot` or `euclidean`.

### Embeddings and advisory queries

Two system processes connect a program to the services of a node.
Each one takes the reply channel first and replies `[true, …]` or `[false, code, detail]`.
A call with send params aborts the deploy with `ParamsUnsupported`, and a persistent call aborts it with `PersistentOpCall`.

- `rho:vectordb:embed` embeds one text, or a list of at most 64 texts with at most 64 KiB of text. It replies `[true, vector, model]`, or `[true, [vector…], model]` for a list. The model of a node is `all-MiniLM-L6-v2`, with 384 dimensions.
- `rho:vectordb:ann` takes a mirrored space, a channel of the space, a query and the options `{"k": k}`. It replies `[true, ids, distances, watermark]`: the ids of at most $`k`$ nearest records, their distances, and the last finalized block that the mirror reflects.

The failure codes of `embed` are `BadArgument`, `EmbeddingRateLimited`, `EmbeddingUnavailable` and `NonFiniteEmbedding`.
The failure codes of `ann` are `NotAVectorDbSpace`, `MirrorDisabled`, `DimensionMismatch`, `BadArgument`, `InvalidOption` and `MirrorUnavailable`.
A text query of `ann` can also fail with the codes of the embedder.

Both processes are recorded processes.
The node records each reply, a failure reply included, and replay produces the recorded reply without a call to the embedder or the mirror.
Before it produces the reply, replay makes the checks of `ann` against consensus state, as play does.
Both processes run in native execution.

An id of `ann` is the 32-byte hash of the source produce of a record.
A program passes the ids to a receive with the query param `ids=`.
The exact engine then keeps only the stored records whose id is among them.
A stale id names no record, so it selects nothing.
The exact order of the query decides among the other records, and not the order of the mirror.

### Example

The mirror receives the records of a deploy only after the deploy succeeds.
So the example stores the records in one deploy, and it queries them in a later deploy.
The first deploy makes a space with a mirror, takes a name of the space, and stores three vectors under it.

```rholang
new vdb(`rho:space:hashmap:vectordb`) in {
  for (m <- vdb!?("default"; capacity=4, dimensions=2, mirror="chroma")) {
    for (mn <- m!?("gensym")) {
      @"my-space"!(*m) | @"my-name"!(*mn) |
      mn!([1.0, 0.0]) | mn!([0.0, 1.0]) | mn!([1.0, 1.0])
    }
  }
}
```

The second deploy asks the mirror for the three nearest records, and then it lets the exact engine select among them.

```rholang
new ann(`rho:vectordb:ann`) in {
  for (m <<- @"my-space") { for (mn <<- @"my-name") {
    for (@r <- ann!?(*m, *mn, [1.0, 0.0], {"k": 3})) {
      match r {
        [true, ids, _, _] => { for (@x <- mn ~ ([1.0, 0.0]; ids=ids, k=3)) { @"result"!(x) } }
        [false, code, _]  => { @"result"!(code) }
      }
    }
  } }
}
```

The receive binds the list of the selected vectors, in the exact order of the query.

### How the mirror stays current

The mirror follows the finalized state of the space through four steps:

1. **Journal.** After each committed transaction, the node computes its net writes. A new record that stays is an upsert, and a removed record is a delete. A peek writes nothing.
2. **Publication.** A deploy whose final result has no errors publishes its journal to the writer of the node. A failed deploy publishes nothing.
3. **Writer.** One task applies the batches in order, from a queue of 1,024 batches, with up to 3 attempts. A publication never waits: a full queue or a failed batch marks its collection dirty.
4. **Reconciler.** On each finalized block, the reconciler repairs each collection that is dirty or older than the block. It sets each registered channel to the records of the finalized state, and then it sets the watermark of the collection.

A node that replays a block publishes the deploys of the block to its own mirror.
The mirror reflects the finalized blocks, and `ann` reports the watermark of that view.
After an LFS bootstrap, the records of a channel that the node never journaled are absent until the channel changes.
After a restart, the dirty marks of the writer are lost, so a failed batch is repaired when its channels change again.

### Running a node with the mirror

To run a node with the mirror, do these steps:

1. Build the node with the `chromadb` feature of the rholang crate: `cargo build -p node --release --features rholang/chromadb`.
2. Start a ChromaDB server of the pinned image: `docker compose -f docker/shard.yml --profile chromadb up`.
3. Set `CHROMA_ENDPOINT` for each node. `docker/shard.yml` sets it to `http://chroma-db:8000`.
4. Give each node its own `CHROMA_DATABASE`, so that no node writes the mirror of another node.

The node makes its database on the first call.
Each collection of a space is named `rrs-` and the lowercase hex of the first 16 bytes of the space id.
The reply of `space!?("config")` reports it under the key `mirror`.
A node without the feature, or without an endpoint, has no mirror: its publications drop, and `ann` replies `MirrorUnavailable`.

The repository tests the backend against a real server with `bash scripts/verify-reified/chroma-live.sh`.
The script starts a disposable server of the pinned image, and it gives each run a new database.
