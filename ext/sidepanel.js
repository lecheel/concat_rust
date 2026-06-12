// Global placeholder for retrieved data to bind to the drag payload
let currentFetchedText = '';

document.addEventListener('DOMContentLoaded', () => {

  const pasteFileRow = document.getElementById('pasteFileRow');
  const pasteFileBox = document.getElementById('pasteFileBox');

  if (pasteFileRow && pasteFileBox) {
    pasteFileRow.addEventListener('click', () => {
      const isChecked = pasteFileBox.classList.toggle('checked');
      pasteFileBox.textContent = isChecked ? '✓' : '';
    });
  }
   
  function updateHeaderTitle() {
    const activeRepoInput = document.getElementById('activeRepo');
    const headerTitle = document.getElementById('headerTitle');
    if (!headerTitle) return;

    const repo = activeRepoInput ? activeRepoInput.value.trim() : '';
    headerTitle.textContent = repo
      ? `Concat Rust Paster (${repo})`
      : 'Concat Rust Paster';
  }

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

  const hostInput = document.getElementById('host');
  const portInput = document.getElementById('port');
  const activeRepoInput = document.getElementById('activeRepo');

  chrome.storage.local.get(['host', 'port', 'activeRepo'], (result) => {
    if (result.host && hostInput) hostInput.value = result.host;
    if (result.port && portInput) portInput.value = result.port;
    if (result.activeRepo && activeRepoInput) activeRepoInput.value = result.activeRepo;
    updateHeaderTitle();
    loadRepos();
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

  if (activeRepoInput) {
    activeRepoInput.addEventListener('input', () => {
      updateHeaderTitle();
      highlightActiveChip();
    });
  }

  const refreshBtn = document.getElementById('refreshRepos');
  if (refreshBtn) {
    refreshBtn.addEventListener('click', loadRepos);
  }

  const dragChip = document.getElementById('dragChip');
  if (dragChip) {
    dragChip.addEventListener('dragstart', (e) => {
      if (!currentFetchedText) {
        e.preventDefault();
        return;
      }
      
      e.dataTransfer.clearData();

      // 1. Resolve dynamic target filename (Skeleton set to .txt)
      let filename = 'skeleton.txt';
      const isSkeleton = document.getElementById('skeletonBox')?.classList.contains('checked') || false;
      const activeRepoInput = document.getElementById('activeRepo');
      const activeRepo = activeRepoInput ? activeRepoInput.value.trim() : '';

      if (isSkeleton) {
        filename = activeRepo ? `${activeRepo}_skeleton.txt` : 'skeleton.txt';
      } else {
        const filesInput = document.getElementById('files');
        const filesText = filesInput ? filesInput.value.trim() : '';
        if (filesText) {
          const firstFile = filesText.split(/[\s,]+/)[0];
          if (firstFile) {
            filename = firstFile.split('/').pop() || 'file.rs';
          }
        } else {
          const hashesInput = document.getElementById('hashes');
          const hashesText = hashesInput ? hashesInput.value.trim() : '';
          if (hashesText) {
            const firstHash = hashesText.split(/[\r\n\s,]+/)[0];
            if (firstHash) {
              filename = `block_${firstHash.substring(0, 8)}.rs`;
            }
          }
        }
      }

      if (!filename.includes('.')) {
        filename += '.rs';
      }

      const mimeType = 'text/plain';
      const blob = new Blob([currentFetchedText], { type: mimeType });
      const blobUrl = URL.createObjectURL(blob);

      const file = new File([currentFetchedText], filename, { type: mimeType });
      e.dataTransfer.items.add(file);

      const downloadUrlData = `${mimeType}:${filename}:${blobUrl}`;
      e.dataTransfer.setData('DownloadURL', downloadUrlData);
      
      e.dataTransfer.effectAllowed = 'copy';

      dragChip.style.opacity = '0.5';

      dragChip.addEventListener('dragend', function cleanup() {
        URL.revokeObjectURL(blobUrl);
        dragChip.style.opacity = '1';
        dragChip.removeEventListener('dragend', cleanup);
      });
    });
  }

  async function loadRepos() {
    const btn = document.getElementById('refreshRepos');
    const chipsContainer = document.getElementById('repoChips');
    if (!btn || !chipsContainer) return;

    const host = (hostInput ? hostInput.value.trim() : '') || '127.0.0.1';
    const port = (portInput ? portInput.value.trim() : '') || '7890';

    btn.classList.add('spinning');
    btn.textContent = '…';

    try {
      const response = await chrome.runtime.sendMessage({
        action: 'fetchRepos',
        host,
        port
      });

      if (response && response.success && Array.isArray(response.data)) {
        renderRepoChips(response.data);
      } else {
        const errMsg = response ? response.error : 'Unknown error';
        chipsContainer.innerHTML = `<span class="repo-chips-empty">⚠ ${escapeHtml(errMsg)}</span>`;
      }
    } catch (err) {
      chipsContainer.innerHTML = `<span class="repo-chips-empty">⚠ ${escapeHtml(err.message)}</span>`;
    } finally {
      btn.classList.remove('spinning');
      btn.textContent = '↻';
    }
  }

  function renderRepoChips(repos) {
    const chipsContainer = document.getElementById('repoChips');
    const activeRepoInput = document.getElementById('activeRepo');
    if (!chipsContainer || !activeRepoInput) return;

    const currentRepo = activeRepoInput.value.trim();

    if (repos.length === 0) {
      chipsContainer.innerHTML = '<span class="repo-chips-empty">No repos registered. Use: cli add-repo &lt;id&gt; &lt;path&gt;</span>';
      return;
    }

    chipsContainer.innerHTML = '';
    for (const repo of repos) {
      const id = repo.id || '?';
      const branch = repo.git_branch || 'detached';
      const files = repo.file_count != null ? repo.file_count : '?';

      const chip = document.createElement('div');
      chip.className = 'repo-chip' + (id === currentRepo ? ' active' : '');
      chip.dataset.repoId = id;
      chip.title = `${id} [${branch}] (${files} files)`;
      chip.innerHTML = `${escapeHtml(id)} <span class="chip-branch">${escapeHtml(branch)}</span>`;

      chip.addEventListener('click', () => {
        if (activeRepoInput.value.trim() === id) {
          activeRepoInput.value = '';
        } else {
          activeRepoInput.value = id;
        }
        chrome.storage.local.set({ activeRepo: activeRepoInput.value.trim() });
        updateHeaderTitle();
        highlightActiveChip();
      });

      chipsContainer.appendChild(chip);
    }
  }

  function highlightActiveChip() {
    const chipsContainer = document.getElementById('repoChips');
    const activeRepoInput = document.getElementById('activeRepo');
    if (!chipsContainer || !activeRepoInput) return;

    const currentRepo = activeRepoInput.value.trim();
    const chips = chipsContainer.querySelectorAll('.repo-chip');
    chips.forEach(chip => {
      chip.classList.toggle('active', chip.dataset.repoId === currentRepo);
    });
  }

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
          highlightActiveChip();
        }
      }
    });
  }
});

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

function hasRepoPrefix(path) {
  const slashPos = path.indexOf('/');
  if (slashPos === -1) return false;
  const first = path.substring(0, slashPos);
  return !first.includes('.') && first !== 'src';
}

function resolvePath(input, activeRepo) {
  let withSrc = shouldAutoPrefixSrc(input) ? `src/${input}` : input;

  if (activeRepo) {
    const repoPrefix = `${activeRepo}/`;
    if (withSrc.startsWith(repoPrefix) || hasRepoPrefix(withSrc)) {
      return withSrc;
    }
    return `${repoPrefix}${withSrc}`;
  }
  return withSrc;
}

function setStatus(type, html) {
  const box     = document.getElementById('statusBox');
  const spinner = document.getElementById('spinner');
  const content = document.getElementById('statusContent');
  if (!box || !spinner || !content) return;
  box.className = 'status-box ' + type;
  spinner.style.display = type === 'loading' ? '' : 'none';
  content.innerHTML = html;
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

  // Read toggle state directly from DOM
  const pasteFileChecked = document.getElementById('pasteFileBox')?.classList.contains('checked') || false;

  // Resolve target file name (Skeleton set to .txt)
  let filename = 'skeleton.txt';
  if (skeletonChecked) {
    params.set('skeleton', 'true');
    if (activeRepo) params.set('repo', activeRepo);
    filename = activeRepo ? `${activeRepo}_skeleton.txt` : 'skeleton.txt';
  } else {
    const hashes = hashesText.split(/[\r\n\s,]+/)
      .map(h => h.trim().replace(/^hash[:=]/i, ''))
      .filter(Boolean);

    const rawFiles = filesText.split(/[\s,]+/).map(f => f.trim()).filter(Boolean);
    const resolvedFiles = rawFiles.map(f => resolvePath(f, activeRepo));

    if (hashes.length === 0 && resolvedFiles.length === 0) {
      setStatus('error', 'Add at least one hash, file path, or enable skeleton.');
      return;
    }
    hashes.forEach(h => params.append('hash', h));
    resolvedFiles.forEach(f => params.append('file', f));

    // Name the file after the target or block hash
    if (rawFiles.length > 0) {
      filename = rawFiles[0].split('/').pop() || 'file.rs';
    } else if (hashes.length > 0) {
      filename = `block_${hashes[0].substring(0, 8)}.rs`;
    }
  }

  if (!filename.includes('.')) {
    filename += '.rs';
  }

  const btn = document.getElementById('fetchBtn');
  if (btn) btn.disabled = true;
  setStatus('loading', 'Fetching from daemon…');

  try {
    const response = await chrome.runtime.sendMessage({
      action: 'fetchAndPaste',
      host,
      port,
      params: params.toString(),
      pasteAsFile: pasteFileChecked,  // Pass setting parsed from DOM
      filename: filename              // Pass filename
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
 
function parseCommandLine(line) {
  const tokens = line.trim().split(/\s+/);
  if (tokens.length === 0 || (tokens.length === 1 && tokens[0] === "")) return null;

  let startIndex = 0;
  const firstToken = tokens[0].toLowerCase();

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
      if (i + 1 < tokens.length && !tokens[i + 1].startsWith('-')) {
        repo = tokens[++i].replace(/['"]/g, '');
      }
    } else if (token.startsWith('-')) {
      parsingFiles = false;
      if (i + 1 < tokens.length && !tokens[i + 1].startsWith('-')) {
        i++;
      }
    } else {
      const parts = token.split(',').map(p => p.replace(/['"]/g, '').trim()).filter(Boolean);
      for (let cleanToken of parts) {
        if (cleanToken) {
          if (/^hash[:=]/i.test(cleanToken)) {
            cleanToken = cleanToken.replace(/^hash[:=]/i, '');
          }

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
