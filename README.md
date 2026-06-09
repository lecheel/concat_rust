# concat_rust

**Provide the overall architecture first, then retrieve specific implementations on demand.**

---

## Motivation

Most real-world Rust codebases are too large for free-tier web-based AI chats (such as the web interfaces for Claude, ChatGPT, or Gemini). If you paste your entire codebase, you will quickly hit context window limits, trigger silent truncation, or deplete your high-quality message quota. Conversely, if you paste only isolated files, the model lacks the structural context of your architecture and often struggles to align with your project's traits, module boundaries, and type definitions.

`concat_rust` was designed specifically for developers using **free-tier web interfaces** or **budget-constrained API endpoints**. It solves this dilemma by splitting your codebase into two parts:
1. A lightweight **architectural skeleton** that fits easily within any free-tier context window.
2. A local **on-demand retrieval daemon** that lets you fetch and paste exact implementations only when the model requests them.

This minimizes token consumption, preserves your daily message allowance, and provides the model with the precise structural context it needs to generate usable Rust code.

---

## How It Works

Using `concat_rust` splits your project representation into a structural outline and granular details:

```
┌──────────────┐         ┌──────────────────┐         ┌─────────────────┐
│  Your Rust   │  strip  │                  │  fetch  │   AI Web Chat   │
│  codebase    │ ──────► │  Skeleton +      │ ◄─────► │  (Free Tier)    │
│  (src/)      │  & hash │  Local Daemon    │  hash/  │  Sees structure │
│              │         │  localhost:7890  │  file   │  Requests details
└──────────────┘         └──────────────────┘         └─────────────────┘
```

1. **Structural Outline**: Function bodies, structs, enums, and traits are stubbed with unique hash identifiers:
   ```rust
   // Before:
   fn process_order(order: &Order) -> Result<Receipt, Error> {
       let validated = order.validate()?;
       let total = validated.items.iter().map(|i| i.price).sum();
       let tax = calculate_tax(total, &validated.region);
       let receipt = Receipt::new(validated, total, tax);
       database::persist(&receipt)?;
       notify_customer(&receipt)?;
       Ok(receipt)
   }

   // After:
   fn process_order(order: &Order) -> Result<Receipt, Error> { /* HASH:a1b2c3d4 */ }
   ```
2. **Context Preservation**: The model learns the function signature and its module location. When it needs to inspect the logic, it requests `a1b2c3d4`.
3. **Targeted Fetching**: You retrieve and share only the implementation requested, keeping the chat session highly focused.

---

## Why This Helps Free-Tier Users

Free-tier web interfaces for AI models often impose tight constraints on both context window size and the number of messages allowed per session. This tool helps optimize both constraints:

| Problem in Free-Tier Web Chats | How `concat_rust` Addresses It |
| :--- | :--- |
| **Small Context Windows** | The skeleton is typically 5–15% of the original codebase size, fitting easily within standard limits. |
| **Strict Message Limits** | Sharing the entire architecture upfront reduces the need for back-and-forth clarification, saving your daily message quota. |
| **Tedious Manual Pasting** | The CLI copies code directly to your clipboard, and the Chrome extension pastes it directly into the chat interface. |
| **Out-of-Context Hallucinations** | Because the model can see all trait bounds and type definitions in the skeleton, it is less likely to guess or invent non-existent APIs. |
| **Performance Degradation** | Keeping the active context small and highly relevant helps the model provide more focused and accurate code generations. |

### Typical Compression Ratio Example

* **Original Project**: ~12,400 lines of code (~52,000 tokens) — *often exceeds practical free-tier context limits.*
* **Skeleton**: ~1,800 lines of code (~7,500 tokens) — *well within typical free-tier limits.*
* **Per-Fetch Body**: ~50–200 lines (~200–800 tokens) — *minimal token overhead for targeted updates.*

---

## Quick Start

### 1. Build the Binaries

This project compiles into two binaries: the main compressor/daemon and a CLI client.

```bash
cargo build --release
```

### 2. Compress and Start the Daemon

Scan your source directory, write the skeleton file, and launch the retrieval daemon on a local port:

```bash
concat_rust --dir src --output skeleton.rs --compress --daemon-port 7890
```

### 3. Copy the Skeleton

Use the CLI client to copy the generated skeleton structure directly to your clipboard, then share it with the model:

```bash
concat_rust_cli --skeleton
```

### 4. Retrieve Requested Implementations

When the model asks to inspect a specific function or file, fetch it using the CLI client:

```bash
# Retrieve by individual hashes
concat_rust_cli a1b2c3d4 e5f67890

# Retrieve complete files for broader context
concat_rust_cli --file src/main.rs src/db.rs

# Combine both methods
concat_rust_cli a1b2c3d4 --file src/models.rs
```

### 5. Resuming an Existing Session

To restart the daemon without re-processing your source files:

```bash
concat_rust --compress --resume --daemon-port 7890
```

---

## Typical Workflow

1. Run the local compressor: `concat_rust --compress --dir src`
2. Copy and paste the structural skeleton into your chat session.
3. The model reviews the structure and requests specific files or function hashes (e.g., *"I need the implementation for `process_order` (hash `a1b2c3d4`) and the file `src/parser.rs`"*).
4. Run `concat_rust_cli a1b2c3d4 --file src/parser.rs` to fetch and paste the requested blocks.
5. The model provides targeted code updates that respect your project's overall architecture.

---

## Chrome Extension

A companion Chrome extension is available in the `ext/` directory, allowing you to fetch and paste directly within web-based chat interfaces (such as Claude, ChatGPT, or Gemini).

### Installation
1. Navigate to `chrome://extensions/` in your browser.
2. Enable **Developer mode** (top-right toggle).
3. Click **Load unpacked** and select the `ext/` folder.

### Features
* **Side Panel**: Input hashes or paths to quickly fetch and paste.
* **Context Menu**: Highlight a hash on the page, right-click, and select "Fetch this hash".
* **Persistent Settings**: Configured daemon host and port settings are preserved.

---

## Compression Details

The compressor strips comments, empty lines, and test modules (`#[cfg(test)]`) to minimize size. Items are reduced as follows:

| Rust Item | Representation in Skeleton |
| :--- | :--- |
| `fn` | Signature + `{ /* HASH:xxxx */ }` |
| `impl` blocks | Outer block signature kept; inner method bodies stubbed to hashes. |
| `struct` | `/* HASH:xxxx (struct Name) */` |
| `enum` | `/* HASH:xxxx (enum Name) */` |
| `trait` | `/* HASH:xxxx (trait Name) */` |
| `use`, `type`, `const`, `mod` | Retained verbatim. |

---

## Daemon API Reference

The local background daemon exposes the following HTTP endpoints:

| Endpoint | Method | Description |
| :--- | :--- | :--- |
| `/skeleton` | `GET` | Returns the complete structural skeleton. |
| `/:hash` | `GET` | Returns the source body matching the given hash (supports prefix matches). |
| `/file/*path` | `GET` | Returns the cleaned, stripped contents of the file at the given path. |

#### Examples:
```bash
curl http://localhost:7890/skeleton
curl http://localhost:7890/a1b2c3d4
curl http://localhost:7890/file/src/main.rs
```

---

## CLI Argument Reference

### `concat_rust` (Daemon & Compressor)

```
Flags:
  --dir <DIR>            Source directory containing your Rust code [default: src]
  --output <FILE>        File path where the skeleton is written [default: output.rs]
  --max-width <WIDTH>    Maximum line width configuration for rustfmt [default: 350]
  --no-format            Do not format output with rustfmt
  --single-line          Concatenate outputs into a single line (no-compress mode)
  --compress             Enable structural compression and launch the daemon
  --daemon-port <PORT>   Port on which the daemon listens [default: 7890]
  --resume               Start the daemon using cached metadata without re-compressing
```

### `concat_rust_cli` (Client)

```
Arguments:
  [HASHES]...            One or more function or type hashes to retrieve

Flags:
  --file <FILES>...      File paths to retrieve
  --skeleton             Fetch the complete project skeleton
  --host <HOST>          Target daemon host address [default: 127.0.0.1]
  --port <PORT>          Target daemon port [default: 7890]
```

---

## License

This project is licensed under the [MIT License](LICENSE).
