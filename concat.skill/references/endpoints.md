# concat_rust REST API Reference

Base URL: `http://127.0.0.1:<PORT>` where PORT is the daemon's configured port default port:7890 .

## Read Endpoints

| Method | Path | Description |
|--------|------|-------------|
| GET | `/` | Dashboard (HTML) |
| GET | `/skeleton` | Compressed code skeleton of all indexed repos |
| GET | `/meta-prompt` | Instruction meta-prompt for LLM context |
| GET | `/catalog` | All files with LOC and sizes |
| GET | `/<hash>` | Body code by content hash |
| GET | `/info/<hash>` | Body metadata by content hash |
| GET | `/file-info/*path` | File metadata by path |
| GET | `/file/*path` | Full file content by path |

## Common Response Formats

### `/skeleton`
Returns a compressed tree structure of all indexed code. Useful for getting a high-level view of codebase structure without full file contents.

### `/catalog`
Returns all indexed files with metadata (lines of code, file sizes). Good for understanding what code is available.

### `/file/<path>`
Returns the full content of a specific file. Path should be relative to the repo root or absolute within the indexed structure.

### `/<hash>`
Returns a deduplicated code body by its SHA hash. The concat_rust daemon deduplicates identical code blocks across files.

### `/meta-prompt`
Returns the instruction meta-prompt that describes how to interpret the skeleton/catalog format. Useful when building LLM prompts from the codebase.
