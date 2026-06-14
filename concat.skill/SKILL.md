---
name: concat-rust
description: Interact with a local concat_rust daemon to retrieve code context from indexed repositories. Use when the user wants to read, explore, or analyze code from local projects that are indexed by the concat_rust service. Triggers include requests like "get code from my repo", "show me the skeleton of project X", "read file X from repo Y", "get the catalog of indexed files", "what's the meta-prompt", or any task requiring code context from concat_rust-indexed repositories.
---

# concat_rust Skill

Interact with a local concat_rust daemon that indexes and serves code from registered repositories via REST API and CLI.

## Prerequisites

- concat_rust daemon must be running locally (usually on `127.0.0.1:<port>`)
- The `cli` binary should be available in PATH, or use the API client script

## Workflow

### 1. Discover daemon port and status

The daemon prints its port on startup. Common ways to find it:
- Check if `cli` works: `cli repos`

### 2. Get skeleton (structural overview)

Always start by fetching the skeleton to understand the codebase structure:

```bash
cli skeleton
```

The skeleton shows all files with their structural blocks and HASH identifiers. Use it to identify which files and blocks are relevant to the task.

### 3. Analyze and plan code requests

Analyze the feature requirements against the skeleton. Determine which files, structs, traits, and functions you need to see the full implementation of.

**Request strategy:**
- Prefer asking for **whole files** rather than individual hashes
- If a file is too large, ask for specific `impl` blocks or struct definitions by their HASH (shown as `/* HASH:1a12fb93 [183 LOC] */` in the skeleton)
- List exactly what you need in a clear, numbered list with:
  - The file path (e.g., `src/main.rs`)
  - If requesting a specific block, its HASH
  - A brief reason (e.g., "to know the fields of AppState", "to see how sync is implemented")

**Do not guess or stub missing implementations. Do not proceed until all requested code is received.**

### 4. Fetch code — batch request

Request everything needed in a single batch command:

```bash
cli file <path1> <path2>
cli hash <hash1> <hash2>
```

Examples:
```bash
cli src/main.rs src/lib.rs
```

### 5. Common utility operations

**List registered repos:**
```bash
cli repos
```

**Get catalog (all files with LOC/sizes):**
```bash
cli catalog
```

**Get meta-prompt (daemon's instruction prompt):**
```bash
cli prompt
```

## Reference Files

- **`references/endpoints.md`** — Full REST API endpoint listing with response formats. Read when you need details on a specific endpoint.
- **`references/cli_reference.md`** — CLI command listing with workflow examples. Read when the CLI is preferred over the API.

## Key Design Notes

- The daemon deduplicates code bodies by hash. Two identical functions will share the same hash.
- `/skeleton` gives a lightweight structural overview — always use it to plan requests.
- `/catalog` shows all indexed files with LOC and sizes.
- `/meta-prompt` returns the daemon's own instruction prompt for LLM context formatting.
- Batch requests with `cli file <path1> <path2> or cli hash <hash1> <hash2>` to minimize round-trips.
