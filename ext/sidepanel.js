document.addEventListener('DOMContentLoaded', () => {
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

  // ── Restore saved host/port ──
  const hostInput = document.getElementById('host');
  const portInput = document.getElementById('port');

  chrome.storage.local.get(['host', 'port'], (result) => {
    if (result.host && hostInput) hostInput.value = result.host;
    if (result.port && portInput) portInput.value = result.port;
  });

  ['host', 'port'].forEach(id => {
    const inputEl = document.getElementById(id);
    if (inputEl) {
      inputEl.addEventListener('change', () => {
        chrome.storage.local.set({
          host: hostInput ? hostInput.value : '127.0.0.1',
          port: portInput ? portInput.value : '7890'
        });
      });
    }
  });

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
      }
    });
  }
});

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

  const hashesText = hashesInput ? hashesInput.value : '';
  const filesText  = filesInput ? filesInput.value : '';
  let host = (hostInput ? hostInput.value.trim() : '') || '127.0.0.1';
  let port = (portInput ? portInput.value.trim() : '') || '7890';

  const params = new URLSearchParams();

  const skeletonChecked = document.getElementById('skeletonBox')?.classList.contains('checked') || false;

  if (skeletonChecked) {
    params.set('skeleton', 'true');
  } else {
    const hashes = hashesText.split(/[\r\n\s]+/).map(h => h.trim()).filter(Boolean);
    const files  = filesText.split(/[\s,]+/).map(f => f.trim()).filter(Boolean);
    if (hashes.length === 0 && files.length === 0) {
      setStatus('error', 'Add at least one hash, file path, or enable skeleton.');
      return;
    }
    hashes.forEach(h => params.append('hash', h));
    files.forEach(f => params.append('file', f));
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
 * Normalizes and extracts parameters from command lines such as:
 * - cli --file main.rs lib.rs
 * - cli hash1 hash2
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

  for (let i = startIndex; i < tokens.length; i++) {
    const token = tokens[i];
    if (!token) continue;

    if (token === '--file' || token === '-f') {
      parsingFiles = true;
    } else if (token === '--skeleton' || token === '-s') {
      skeleton = true;
    } else if (token.startsWith('-')) {
      parsingFiles = false;
      if (i + 1 < tokens.length && !tokens[i + 1].startsWith('-')) {
        i++; 
      }
    } else {
      const cleanToken = token.replace(/['",]/g, '').trim();
      if (cleanToken) {
        if (parsingFiles) {
          files.push(cleanToken);
        } else {
          hashes.push(cleanToken);
        }
      }
    }
  }

  return { hashes, files, skeleton };
}

function escapeHtml(str) {
  return String(str)
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;');
}
