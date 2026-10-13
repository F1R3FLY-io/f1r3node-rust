# System Channels

All built-in system channels available in the Rust shard. Verified against `system_processes.rs` and `rho_runtime.rs`.

Bind system channels using URI syntax inside `new`:

```rho
new stdout(`rho:io:stdout`),
    sha256(`rho:crypto:sha256Hash`) in {
  ...
}
```

## I/O Channels

### `rho:io:stdout` (arity 1)

Print to standard output. No acknowledgment.

```rho
new stdout(`rho:io:stdout`) in {
  stdout!("Hello, World!")
  stdout!(42)
  stdout!([1, 2, 3])
}
```

### `rho:io:stdoutAck` (arity 2)

Print to standard output with acknowledgment. Use for sequencing output.

```rho
new stdoutAck(`rho:io:stdoutAck`) in {
  new ack in {
    stdoutAck!("first", *ack) |
    for (_ <- ack) {
      stdoutAck!("second", *ack)
    }
  }
}
```

### `rho:io:stderr` (arity 1)

Print to standard error. No acknowledgment.

```rho
new stderr(`rho:io:stderr`) in {
  stderr!("Error occurred")
}
```

### `rho:io:stderrAck` (arity 2)

Print to standard error with acknowledgment.

```rho
new stderrAck(`rho:io:stderrAck`) in {
  new ack in {
    stderrAck!("Error details", *ack)
  }
}
```

## Cryptographic Channels

### `rho:crypto:sha256Hash` (arity 2)

SHA-256 hash. Input: ByteArray. Output: ByteArray (32 bytes).

```rho
new sha256(`rho:crypto:sha256Hash`) in {
  new ret in {
    sha256!("hello".toUtf8Bytes(), *ret) |
    for (@hash <- ret) {
      stdout!(hash.bytesToHex())
    }
  }
}
```

### `rho:crypto:keccak256Hash` (arity 2)

Keccak-256 hash. Input: ByteArray. Output: ByteArray (32 bytes).

```rho
new keccak(`rho:crypto:keccak256Hash`) in {
  new ret in {
    keccak!("hello".toUtf8Bytes(), *ret) |
    for (@hash <- ret) {
      stdout!(hash.bytesToHex())
    }
  }
}
```

### `rho:crypto:blake2b256Hash` (arity 2)

Blake2b-256 hash. Input: ByteArray. Output: ByteArray (32 bytes).

```rho
new blake(`rho:crypto:blake2b256Hash`) in {
  new ret in {
    blake!("hello".toUtf8Bytes(), *ret) |
    for (@hash <- ret) {
      stdout!(hash.bytesToHex())
    }
  }
}
```

### `rho:crypto:ed25519Verify` (arity 4)

Ed25519 signature verification.

```rho
new ed25519(`rho:crypto:ed25519Verify`) in {
  new ret in {
    ed25519!(data, signature, publicKey, *ret) |
    for (@valid <- ret) {
      // valid is true or false
    }
  }
}
```

Args: data (ByteArray), signature (ByteArray), publicKey (ByteArray), ack channel.
Returns: Boolean.

### `rho:crypto:secp256k1Verify` (arity 4)

Secp256k1 (ECDSA) signature verification. Same calling convention as ed25519Verify.

```rho
new secp(`rho:crypto:secp256k1Verify`) in {
  new ret in {
    secp!(data, signature, publicKey, *ret) |
    for (@valid <- ret) {
      // valid is true or false
    }
  }
}
```

## Block and Deploy Data

### `rho:block:data` (arity 1)

Get current block metadata.

```rho
new blockData(`rho:block:data`) in {
  new ret in {
    blockData!(*ret) |
    for (@blockNumber, @timestamp, @senderPubKey <- ret) {
      stdout!(blockNumber)
    }
  }
}
```

Returns 3-tuple: (blockNumber: Int, timestamp: Int, senderPublicKey: ByteArray).

### `rho:deploy:data` (arity 1)

Get current deploy metadata.

```rho
new deployData(`rho:deploy:data`) in {
  new ret in {
    deployData!(*ret) |
    for (@timestamp, @deployerId, @deployId <- ret) {
      stdout!(deployerId)
    }
  }
}
```

Returns 3-tuple: (timestamp: Int, deployerId, deployId).

## Vault and Identity

### `rho:vault:address` (arity 3)

Vault address operations. Command-based dispatch.

```rho
new vaultAddr(`rho:vault:address`) in {
  new ret in {
    // Compute vault address from public key
    vaultAddr!("fromPublicKey", publicKeyBytes, *ret) |
    for (@address <- ret) {
      stdout!(address)
    }
  }
}
```

Commands:
- `"validate"` -- validate an address string
- `"fromPublicKey"` -- derive address from public key bytes
- `"fromDeployerId"` -- derive address from deployer ID
- `"fromUnforgeable"` -- derive address from unforgeable name

### `rho:system:deployerId:ops` (arity 3)

Deployer identity operations.

```rho
new deployerOps(`rho:system:deployerId:ops`) in {
  new ret in {
    deployerOps!("pubKeyBytes", deployerId, *ret) |
    for (@pubKey <- ret) {
      stdout!(pubKey.bytesToHex())
    }
  }
}
```

Commands: `"pubKeyBytes"` -- extract public key bytes from deployer ID.

### `sys:authToken:ops` (arity 3)

System authentication token verification.

```rho
new authOps(`sys:authToken:ops`) in {
  new ret in {
    authOps!("check", token, *ret) |
    for (@valid <- ret) {
      // valid is boolean
    }
  }
}
```

Commands: `"check"` -- verify a system auth token.

## Registry

### `rho:registry:lookup` (bundled)

Look up a registered process by URI.

```rho
new lookup(`rho:registry:lookup`) in {
  new ret in {
    lookup!(`rho:id:someuri`, *ret) |
    for (value <- ret) {
      stdout!(*value)
    }
  }
}
```

### `rho:registry:insertArbitrary` (bundled)

Register a process and get back a URI.

```rho
new insert(`rho:registry:insertArbitrary`) in {
  new ret, myContract in {
    contract myContract(@msg, ack) = { ack!(msg) } |
    insert!(bundle+{*myContract}, *ret) |
    for (@uri <- ret) {
      stdout!(uri)    // prints the generated URI
    }
  }
}
```

### `rho:registry:insertSigned:secp256k1` (bundled)

Register with cryptographic signature verification.

### `rho:registry:ops` (arity 3)

Registry operations.

```rho
new regOps(`rho:registry:ops`) in {
  new ret in {
    regOps!("buildUri", data, *ret) |
    for (@uri <- ret) {
      stdout!(uri)
    }
  }
}
```

Commands: `"buildUri"` -- construct a registry URI.

## Consensus

### `rho:casper:invalidBlocks` (arity 1)

Query the current set of invalid blocks.

```rho
new invalid(`rho:casper:invalidBlocks`) in {
  new ret in {
    invalid!(*ret) |
    for (@blocks <- ret) {
      stdout!(blocks)
    }
  }
}
```

## Network

### `rho:io:grpcTell` (arity 3)

Fire-and-forget gRPC notification. No acknowledgment.

```rho
new grpcTell(`rho:io:grpcTell`) in {
  grpcTell!("localhost", 8080, {"event": "deploy_complete"})
}
```

Args: host (String), port (Int), payload (any Par).

This is a non-deterministic operation -- results are cached during replay.

### `rho:io:devNull` (arity 1)

Discard all input. Useful as a sink.

```rho
new devNull(`rho:io:devNull`) in {
  devNull!("this goes nowhere")
}
```

### `rho:execution:abort` (arity 1)

Immediately terminate execution with an error.

```rho
new abort(`rho:execution:abort`) in {
  abort!("fatal error: invalid state")
}
```

## AI/ML Integration

All AI channels are non-deterministic. During replay, cached results are returned.

### OpenAI Channels

#### `rho:ai:gpt4` (arity 2)

```rho
new gpt4(`rho:ai:gpt4`) in {
  new ret in {
    gpt4!("Explain quantum computing in one sentence", *ret) |
    for (@response <- ret) {
      stdout!(response)
    }
  }
}
```

#### `rho:ai:dalle3` (arity 2)

```rho
new dalle(`rho:ai:dalle3`) in {
  new ret in {
    dalle!("A cat programming in Rholang", *ret) |
    for (@imageUrl <- ret) {
      stdout!(imageUrl)
    }
  }
}
```

#### `rho:ai:textToAudio` (arity 2)

```rho
new tts(`rho:ai:textToAudio`) in {
  new ret in {
    tts!("Hello from the blockchain", *ret) |
    for (@audioBytes <- ret) {
      // audioBytes is a ByteArray
    }
  }
}
```

### Ollama Channels (Local LLM)

See [Ollama Integration](ollama.md) for setup and configuration.

#### `rho:ollama:chat` (arity 3)

```rho
new ollama(`rho:ollama:chat`) in {
  new ret in {
    ollama!("mistral", "What is 2+2?", *ret) |
    for (@response <- ret) {
      stdout!(response)
    }
  }
}
```

Args: model name (String), prompt (String), ack channel.

#### `rho:ollama:generate` (arity 3)

Same as chat but uses the generate endpoint (no conversation context).

```rho
new gen(`rho:ollama:generate`) in {
  new ret in {
    gen!("codellama", "Write a fibonacci function", *ret) |
    for (@response <- ret) {
      stdout!(response)
    }
  }
}
```

#### `rho:ollama:models` (arity 1)

List available local Ollama models.

```rho
new models(`rho:ollama:models`) in {
  new ret in {
    models!(*ret) |
    for (@modelList <- ret) {
      stdout!(modelList)   // list of model name strings
    }
  }
}
```

## <a id="reified-space-factories"></a>Reified RSpaces: factories and space processes

[Chapter 21](21-reified-spaces.md) teaches Reified spaces, and the [agent API](../rspace/reified-rspaces/agent-api.md) states each call.
The processes of Table 1 create spaces and answer questions about them.
A persistent call `!!` of any of them aborts the deploy with `PersistentFactoryCall` or `PersistentOpCall`.

*Table 1. The space processes of Reified RSpaces.*

| URN | Arity | What the process does |
|---|---|---|
| `rho:space:<outer>:<inner>`, one for each of the 32 families | 2 | Creates a space of the family and replies its handle ([chapter 21](21-reified-spaces.md#space-families)) |
| `rho:space:HashMapBagSpace` and `rho:space:PathMapSpace` | 2 | The FIPS names of the factories of `hashmap:bag` and `pathmap:bag` |
| `rho:space:getSpaceAgent` | 2 | Replies the factory of the family of a space |
| `rho:assert:assertEqual` | 3 | Replies `true` when two values are equal after sorting, and `false` otherwise |
| `rho:reified:internal:handleOp`, `rho:reified:internal:matcherOp` and `rho:reified:internal:sinkOp` | 2, 2 and 0 | The internal processes of handles and user agents. Each one accepts only the capabilities that the node builds. |

A factory takes any number of arguments, because the node packs the arguments of a call into one list.
So the arity of a factory counts the reply channel and that list.

## <a id="reified-vectors-and-vectordb-services"></a>Reified RSpaces: vectors and the VectorDB services

The VectorDB family of [Reified RSpaces](21-reified-spaces.md#vectordb-spaces) adds three system processes.
Each one takes the reply channel first and replies `[true, …]`, or `[false, code, detail]` with a fixed detail text.
So `!?` and `try … catch` read a reply.
A call with send params aborts the deploy with `ParamsUnsupported`, and a persistent call `!!` aborts it with `PersistentOpCall`.
The [Reified RSpaces specification](../rspace/reified-rspaces/specification.md#9-system-processes-of-the-vectordb-family) states the rules.
Each code block of this section is the text of an example file, and the examples harness runs each file.

### `rho:lang:vector` (call `vec!(ret, op, x…)`)

The process computes with vectors exactly.
The operation `op` is `"add"`, `"scale"`, `"dot"`, `"norm"` or `"distance"`.

```rholang
// rho:lang:vector computes exactly with vectors of numbers. Its operations are
// "add", "scale", "dot", "norm" and "distance". Each reply is [true, value],
// or [false, code] for an error.
//
// The legacy branch had more operations, such as sigmoid, softmax and the
// hyper-dimensional operations. They come with TensorLogic in phase P12
// (decisions 52, 62 to 65 and 88).
new vec(`rho:lang:vector`), out in {
  for (@sum <- vec!?("add", [1, 2], [3, 4])) {
    for (@scaled <- vec!?("scale", [1, 2], 3)) {
      for (@dot <- vec!?("dot", [1, 2], [3, 4])) {
        for (@norm <- vec!?("norm", [3, 4])) {
          for (@distance <- vec!?("distance", [0, 0], [3, 4])) {
            out!([sum, scaled, dot, norm, distance])
          }
        }
      }
    }
  } |
  for (@replies <- out) { @"rrs.example.vector.ops"!(replies) }
}
```

The deploy reports `[[true, [4, 6]], [true, [3, 6]], [true, 11], [true, 5.0], [true, 5.0]]`.
`add`, `scale` and `dot` compute exactly and reply in the widest kind of their inputs.
Integers give an integer, a big rational or a fixed point gives a big rational, and a float gives an `f64`.
The `f64` is one rounding of the exact value, so `dot` rounds its exact sum once.
`norm` and `distance` reply the correctly rounded `f64` of the root of an exact square.
The failure codes are `UnknownMethod`, `BadArgument`, `NotAVector`, `NotANumber`, `NonFiniteValue`, `MalformedNumber`, `DimensionMismatch` and `ValueOutOfRange`.

### `rho:vectordb:embed` (arity 2)

The process embeds a text with the embedding model of the node.
It also takes a list of at most 64 texts with at most 64 KiB of text.

```rholang
// rho:vectordb:embed embeds a text with the embedding model of the node. It
// replies [true, vector, model], or [false, code, detail] for an error. The
// node records the reply, so the replay of the block gives the same reply
// without a call to the model.
//
// The model of a node gives 384 Float32 values. The examples harness installs
// a fake model whose vector of a text has two values.
new embed(`rho:vectordb:embed`) in {
  for (@reply <- embed!?("a short text")) { @"rrs.example.embed"!(reply) }
}
```

With the model of a node, a vector is a list of 384 `Float32` values.
With the fake model of the examples harness, the deploy reports `[true, [12.5f32, 13.5f32], "rrs-examples"]`.
The failure codes are `BadArgument`, `EmbeddingRateLimited`, `EmbeddingUnavailable` and `NonFiniteEmbedding`.
The process is recorded: the node records each reply, and replay produces the recorded reply without a call to the model.

### `rho:vectordb:ann` (arity 5)

The process asks the ChromaDB mirror of the node for the records of a channel of a VectorDB space that are nearest to a query.
The arguments are the reply channel, the space, the channel, the query (a vector or a text) and the options `{"k": k}`.

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

This deploy is the second deploy of the mirror example of [ChromaDB Integration](chromadb.md#reified-chromadb-mirror), whose first deploy stores the records.
The ids of the reply are hints for the query param `ids` of a receive, and the exact engine of the space decides.
The failure codes are `NotAVectorDbSpace`, `MirrorDisabled`, `DimensionMismatch`, `BadArgument`, `InvalidOption` and `MirrorUnavailable`.
A text query can also fail with the codes of `embed`.
Like `embed`, the process is recorded. Replay also makes the checks of the call against consensus state, as play does.

## Channel Summary

| URI | Arity | Category | Deterministic |
|-----|-------|----------|---------------|
| `rho:io:stdout` | 1 | I/O | Yes |
| `rho:io:stdoutAck` | 2 | I/O | Yes |
| `rho:io:stderr` | 1 | I/O | Yes |
| `rho:io:stderrAck` | 2 | I/O | Yes |
| `rho:crypto:ed25519Verify` | 4 | Crypto | Yes |
| `rho:crypto:sha256Hash` | 2 | Crypto | Yes |
| `rho:crypto:keccak256Hash` | 2 | Crypto | Yes |
| `rho:crypto:blake2b256Hash` | 2 | Crypto | Yes |
| `rho:crypto:secp256k1Verify` | 4 | Crypto | Yes |
| `rho:block:data` | 1 | Block | Yes |
| `rho:deploy:data` | 1 | Deploy | Yes |
| `rho:casper:invalidBlocks` | 1 | Consensus | Yes |
| `rho:vault:address` | 3 | Identity | Yes |
| `rho:system:deployerId:ops` | 3 | Identity | Yes |
| `sys:authToken:ops` | 3 | Identity | Yes |
| `rho:registry:lookup` | varies | Registry | Yes |
| `rho:registry:insertArbitrary` | varies | Registry | Yes |
| `rho:registry:insertSigned:secp256k1` | varies | Registry | Yes |
| `rho:registry:ops` | 3 | Registry | Yes |
| `rho:io:grpcTell` | 3 | Network | No |
| `rho:io:devNull` | 1 | Utility | Yes |
| `rho:execution:abort` | 1 | Control | Yes |
| `rho:ai:gpt4` | 2 | AI | No |
| `rho:ai:dalle3` | 2 | AI | No |
| `rho:ai:textToAudio` | 2 | AI | No |
| `rho:ollama:chat` | 3 | AI | No |
| `rho:ollama:generate` | 3 | AI | No |
| `rho:ollama:models` | 1 | AI | No |
| `rho:lang:vector` | varies | Reified | Yes |
| `rho:vectordb:embed` | 2 | Reified | Recorded |
| `rho:vectordb:ann` | 5 | Reified | Recorded |
| `rho:space:*` (the 32 factories and the 2 FIPS names) | 2 | Reified | Yes |
| `rho:space:getSpaceAgent` | 2 | Reified | Yes |
| `rho:assert:assertEqual` | 3 | Reified | Yes |
| `rho:reified:internal:*` (3 processes) | 2, 2 and 0 | Reified | Yes |
