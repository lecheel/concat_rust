### Base URL
`http://127.0.0.1:7890` (Default host and port)

---

### 1. Core Code Context Endpoints
These endpoints are used to fetch the actual code and skeleton to feed into an LLM context window.

#### `GET /skeleton`
Fetches the compressed code skeleton for the active repo(s).
*   **Query Params:** `repo` (string, optional) - Target a specific repository (e.g., `?repo=core-api`). If omitted, fetches for all repos.
*   **Response (200 OK):** `text/plain` (The compressed skeleton)
*   **Headers:** 
    *   `x-loc`: Total lines of code in the returned skeleton.
    *   `x-repo`: The repo context used.
*   **Integration Note:** Calling this endpoint **resets** the daemon's session LOC counters (accessible via `/loc-info`).

#### `GET /file/*path`
Fetches the full, uncompressed source code of a specific file.
*   **Query Params:** `repo` (string, optional) - Repo context.
*   **Path Param:** `*path` (string) - The file path (e.g., `/file/src/main.rs`).
*   **Response (200 OK):** `text/plain` (The file content, prefixed with `//--+ file:///path`)
*   **Headers:**
    *   `x-loc`: Lines of code in the file.
    *   `x-byte-size`: File size in bytes.
    *   `x-source`: `cache` (if indexed Rust file) or `disk` (if read directly).
    *   `x-filepath`: The resolved display path.

#### `GET /:hash` (or `/:hash1+:hash2`)
Fetches the full implementation of a specific code block (struct, impl, function) by its hash. Supports fetching multiple at once.
*   **Path Param:** `:hash` (string) - A single hash, or multiple hashes joined by `+` or `,` (e.g., `/a1b2c3d4+e5f6g7h8`).
*   **Query Params:** `repo` (string, optional).
*   **Response (200 OK):** `text/plain` (The code body/bodies, separated by newlines).
*   **Headers:**
    *   `x-loc`: Total lines of code across all returned hashes.
*   **Errors:** `404 Not Found` (invalid hash), `409 Conflict` (hash prefix matched multiple blocks).

---

### 2. Metadata & Stats Endpoints

#### `GET /loc-info`
*(Newly added)* Retrieves the running total of Lines of Code (LOC) fetched in the current session.
*   **Response (200 OK):** `application/json`
    ```json
    {
      "skeleton_loc": 1500,
      "file_loc": 320,
      "hash_loc": 180,
      "total_loc": 2000
    }
    ```
*   **Integration Note:** `skeleton_loc` is set when `/skeleton` is called. `file_loc` and `hash_loc` increment cumulatively with subsequent `/file` and `/:hash` calls. All counters reset to 0 the next time `/skeleton` is called.

#### `GET /catalog`
Lists all indexed files in the daemon cache.
*   **Response (200 OK):** `application/json`
    ```json
    {
      "files": [
        { "filepath": "src/main.rs", "loc": 114, "byte_size": 3500 }
      ]
    }
    ```

#### `GET /file-info/*path`
Gets metadata and extracted AST body hashes for a specific file.
*   **Response (200 OK):** `application/json`
    ```json
    {
      "filepath": "src/main.rs",
      "loc": 114,
      "byte_size": 3500,
      "body_hashes": [
        { "hash": "56532741a4a7", "filepath": "src/main.rs", "loc": 70, "byte_size": 1800 }
      ],
      "source": "cache"
    }
    ```

#### `GET /info/:hash`
Gets metadata for a specific hash (or multiple hashes joined by `+`/`,`).
*   **Response (200 OK):** `application/json` (Array of matching bodies)
    ```json
    [
      { "hash": "56532741a4a7", "filepath": "src/main.rs", "loc": 70, "byte_size": 1800 }
    ]
    ```

#### `GET /stats`
Returns usage statistics (top requested files and hashes per repo).
#### `GET /logs`
Returns recent daemon request logs.
#### `GET /meta-prompt`
Returns the system prompt instructions designed for the LLM to tell it how to request files/hashes.

---

### 3. Repository Management Endpoints

#### `GET /active` (or `/active-repo`)
Gets the ID of the currently active repository.
*   **Response (200 OK):** `text/plain` (e.g., `core-api`, or empty string if none).

#### `GET /repos`
Lists all registered local repositories.
*   **Response (200 OK):** `application/json`
    ```json
    [
      {
        "id": "core-api",
        "source_path": "/Users/dev/projects/core-api",
        "git_branch": "main",
        "file_count": 45,
        "active": true,
        "last_sync": 1690000000
      }
    ]
    ```

#### `POST /repos`
Registers a new repository and triggers an initial background sync.
*   **Request Body:** `application/json`
    ```json
    { "id": "core-api", "source_path": "/Users/dev/projects/core-api" }
    ```
*   **Response (200 OK):** `text/plain` (Success message)

#### `DELETE /repos/:id`
Removes a repository from the daemon registry (does not delete source files).

#### `POST /sync`
Triggers a background sync and re-index for *all* registered repos.
#### `POST /sync/:id`
Triggers a background sync and re-index for a *specific* repo.
