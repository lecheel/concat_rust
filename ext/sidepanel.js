document.addEventListener('DOMContentLoaded', () => {
// ── Dynamic Header Title ──
function updateHeaderTitle() {
  const activeRepoInput = document.getElementById('activeRepo');
  const headerTitle = document.getElementById('headerTitle');
  if (!headerTitle) return;
  
  const repo = activeRepoInput ? activeRepoInput.value.trim() : '';
  headerTitle.textContent = repo 
    ? `Concat Rust Paster (repo ${repo})` 
    : 'Concat Rust Paster';
}    
   
  // ── Skeleton toggle ──
  let skeletonChecked = false;
  const skeletonRow = document.getElementById('skeletonRow');
  const skeletonBox = document.getElementById('skeletonBox');
  const hashSection = document.getElementById('hashSection');
  const fileSection = document.getElementById('fileSection');

  if (skeletonRow && skeletonBox) {
    skeletonRow.addEventListener('click', () => {
      skeletonChecked = !skeletonChecked;
      skeletonBox.classList.toggle('checked', skeletonChecked);
      skeletonBox.textContent = skeletonChecked ? '✓' : '';
      if (hashSection) hashSection.classList.toggle('dimmed', skeletonChecked);
      if (fileSection) fileSection.classList.toggle('dimmed', skeletonChecked);
    });
  }

  // ── Clear buttons ──
  const clearQuickCommandBtn = document.getElementById('clearQuickCommand');
  const quickCommandInput = document.getElementById('quickCommand');
  if (clearQuickCommandBtn && quickCommandInput) {
    clearQuickCommandBtn.addEventListener('click', (e) => {
      e.preventDefault();
      quickCommandInput.value = '';
      quickCommandInput.focus();
    });
  }

  const clearHashesBtn = document.getElementById('clearHashes');
  const hashesInput = document.getElementById('hashes');
  if (clearHashesBtn && hashesInput) {
    clearHashesBtn.addEventListener('click', (e) => {
      e.preventDefault();
      hashesInput.value = '';
      hashesInput.focus();
    });
  }

  const clearFilesBtn = document.getElementById('clearFiles');
  const filesInput = document.getElementById('files');
  if (clearFilesBtn && filesInput) {
    clearFilesBtn.addEventListener('click', (e) => {
      e.preventDefault();
      filesInput.value = '';
      filesInput.focus();
    });
  }


  // ── Restore saved host/port/repo ──
  const hostInput = document.getElementById('host');
  const portInput = document.getElementById('port');
  const activeRepoInput = document.getElementById('activeRepo');

  chrome.storage.local.get(['host', 'port', 'activeRepo'], (result) => {
    if (result.host && hostInput) hostInput.value = result.host;
    if (result.port && portInput) portInput.value = result.port;
    if (result.activeRepo && activeRepoInput) activeRepoInput.value = result.activeRepo;
    
    // Update header title with restored repo
    updateHeaderTitle();
  });

  ['host', 'port', 'activeRepo'].forEach(id => {
    const inputEl = document.getElementById(id);
    if (inputEl) {
      inputEl.addEventListener('change', () => {
        chrome.storage.local.set({
          host: hostInput ? hostInput.value : '127.0.0.1',
          port: portInput ? portInput.value : '7890',
          activeRepo: activeRepoInput ? activeRepoInput.value.trim() : ''
        });
      });
    }
  });

  // Live-update header title as user types in the repo field
  if (activeRepoInput) {
    activeRepoInput.addEventListener('input', updateHeaderTitle);
  }

  // ── Fetch & Paste ──
  const fetchBtn = document.getElementById('fetchBtn');
  if (fetchBtn) {
    fetchBtn.addEventListener('click', doFetch);
  }

  if (hashesInput) {
    hashesInput.addEventListener('keydown', (e) => {
      if (e.key === 'Enter' && (e.ctrlKey || e.metaKey)) {
        e.preventDefault();
        doFetch();
      }
    });
  }
  if (filesInput) {
    filesInput.addEventListener('keydown', (e) => {
      if (e.key === 'Enter') {
        e.preventDefault();
        doFetch();
      }
    });
  }

  // ── Quick Command Parser ──
  if (quickCommandInput) {
    quickCommandInput.addEventListener('input', () => {
      const line = quickCommandInput.value.trim();
      if (!line) return;

      const parsed = parseCommandLine(line);
      if (parsed) {
        if (parsed.hashes.length > 0 && hashesInput) {
          hashesInput.value = parsed.hashes.join('\n');
        } else if (hashesInput) {
          hashesInput.value = '';
        }

        if (parsed.files.length > 0 && filesInput) {
          filesInput.value = parsed.files.join(', ');
        } else if (filesInput) {
          filesInput.value = '';
        }

        if (parsed.skeleton) {
          if (!skeletonChecked && skeletonRow) skeletonRow.click();
        } else {
          if (skeletonChecked && skeletonRow) skeletonRow.click();
        }

        if (parsed.repo && activeRepoInput) {
          activeRepoInput.value = parsed.repo;
          chrome.storage.local.set({ activeRepo: parsed.repo });
          updateHeaderTitle();
        }
      }
    });
  }
});

// ── V2 Path Resolution Logic ─────────────────────────────────

const ROOT_LEVEL_FILES = [
  "Cargo.toml", "Cargo.lock", "docker-compose.yml", "docker-compose.yaml",
  "Dockerfile", ".env", ".env.example", "Makefile", "README.md", "build.rs"
];

function shouldAutoPrefixSrc(path) {
  if (path.startsWith("src/") || path.includes("/src/")) return false;
  const filename = path.split('/').pop();
  if (ROOT_LEVEL_FILES.includes(filename)) return false;
  if (path.includes('/')) return true;
  if (path.endsWith('.rs')) return true;
  return false;
}

function resolvePath(input, activeRepo) {
  let withSrc = shouldAutoPrefixSrc(input) ? `src/${input}` : input;
  
  if (activeRepo) {
    const repoPrefix = `${activeRepo}/`;
    if (withSrc.startsWith(repoPrefix)) {
      return withSrc;
    } else {
      return `${repoPrefix}${withSrc}`;
    }
  }
  return withSrc;
}

// ── Status helpers ──
function setStatus(type, html) {
  const box     = document.getElementById('statusBox');
  const spinner = document.getElementById('spinner');
  const content = document.getElementById('statusContent');
  if (!box || !spinner || !content) return;
  box.className = 'status-box ' + type;
  spinner.style.display = type === 'loading' ? '' : 'none';
  content.innerHTML = html;
}

function clearStatus() {
  const box = document.getElementById('statusBox');
  if (box) box.className = 'status-box';
}

async function doFetch() {
  const hashesInput = document.getElementById('hashes');
  const filesInput  = document.getElementById('files');
  const hostInput   = document.getElementById('host');
  const portInput   = document.getElementById('port');
  const activeRepoInput = document.getElementById('activeRepo');

  const hashesText = hashesInput ? hashesInput.value : '';
  const filesText  = filesInput ? filesInput.value : '';
  let host = (hostInput ? hostInput.value.trim() : '') || '127.0.0.1';
  let port = (portInput ? portInput.value.trim() : '') || '7890';
  const activeRepo = activeRepoInput ? activeRepoInput.value.trim() : '';

  const params = new URLSearchParams();

  const skeletonChecked = document.getElementById('skeletonBox')?.classList.contains('checked') || false;

  if (skeletonChecked) {
    params.set('skeleton', 'true');
    if (activeRepo) {
      params.set('repo', activeRepo);
    }
  } else {
    const hashes = hashesText.split(/[\r\n\s,]+/).map(h => h.trim()).filter(Boolean);
    const rawFiles = filesText.split(/[\s,]+/).map(f => f.trim()).filter(Boolean);
    
    // V2: Resolve file paths with active repo
    const resolvedFiles = rawFiles.map(f => resolvePath(f, activeRepo));
    
    if (hashes.length === 0 && resolvedFiles.length === 0) {
      setStatus('error', 'Add at least one hash, file path, or enable skeleton.');
      return;
    }
    hashes.forEach(h => params.append('hash', h));
    resolvedFiles.forEach(f => params.append('file', f));
  }

  const btn = document.getElementById('fetchBtn');
  if (btn) btn.disabled = true;
  setStatus('loading', 'Fetching from daemon…');

  try {
    const response = await chrome.runtime.sendMessage({
      action: 'fetchAndPaste',
      host,
      port,
      params: params.toString()
    });

    if (response && response.success) {
      const items = response.summaries.map(s =>
        `<div class="status-item">✓ <span>${escapeHtml(s)}</span></div>`
      ).join('');
      setStatus('success',
        `<div class="status-title">Copied to clipboard</div>` +
        `<div class="status-items">${items}</div>`
      );
    } else {
      const errMsg = response ? response.error : 'Unknown response error';
      setStatus('error', escapeHtml(errMsg));
    }
  } catch (err) {
    setStatus('error', 'Communication error: ' + escapeHtml(err.message));
  } finally {
    if (btn) btn.disabled = false;
  }
}

/**
 * V2 Command Line Parser
 * Supports: cli use grab file lib.rs main.rs OR cli 83d650c708ad
 */
function parseCommandLine(line) {
  const tokens = line.trim().split(/\s+/);
  if (tokens.length === 0 || (tokens.length === 1 && tokens[0] === "")) return null;

  let startIndex = 0;
  const firstToken = tokens[0].toLowerCase();
  
  // Skip binary execution prefix
  if (firstToken === 'cli' || firstToken === 'concat-cli' || firstToken.endsWith('cli') || firstToken === 'cargo') {
    startIndex = 1;
    if (firstToken === 'cargo' && tokens[1] && tokens[1].toLowerCase() === 'run') {
      startIndex = 2;
      if (tokens[2] === '--') {
        startIndex = 3;
      }
    }
  }

  const hashes = [];
  const files = [];
  let parsingFiles = false;
  let skeleton = false;
  let repo = '';

  for (let i = startIndex; i < tokens.length; i++) {
    const token = tokens[i];
    if (!token) continue;

    if (token === '--file' || token === '-f' || token.toLowerCase() === 'file') {
      parsingFiles = true;
    } else if (token === '--skeleton' || token === '-s' || token.toLowerCase() === 'skeleton') {
      skeleton = true;
    } else if (token === '--repo' || token === '-r' || token.toLowerCase() === 'use' || token.toLowerCase() === 'repo') {
      // Extract repo name (e.g., 'cli use grab' or 'cli --repo grab')
      if (i + 1 < tokens.length && !tokens[i + 1].startsWith('-')) {
        repo = tokens[++i].replace(/['"]/g, '');
      }
    } else if (token.startsWith('-')) {
      parsingFiles = false;
      if (i + 1 < tokens.length && !tokens[i + 1].startsWith('-')) {
        i++; 
      }
    } else {
      // Split by comma first, then clean each part individually
      const parts = token.split(',').map(p => p.replace(/['"]/g, '').trim()).filter(Boolean);
      for (const cleanToken of parts) {
        if (cleanToken) {
          // Heuristic: if it contains a dot or slash, treat as a file
          if (parsingFiles || cleanToken.includes('.') || cleanToken.includes('/')) {
            files.push(cleanToken);
          } else {
            hashes.push(cleanToken);
          }
        }
      }
    }
  }

  return { hashes, files, skeleton, repo };
}

function escapeHtml(str) {
  return String(str)
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;');
}
