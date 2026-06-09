// ── Skeleton toggle ──
let skeletonChecked = false;
const skeletonRow = document.getElementById('skeletonRow');
const skeletonBox = document.getElementById('skeletonBox');
const hashSection = document.getElementById('hashSection');
const fileSection = document.getElementById('fileSection');

skeletonRow.addEventListener('click', () => {
  skeletonChecked = !skeletonChecked;
  skeletonBox.classList.toggle('checked', skeletonChecked);
  skeletonBox.textContent = skeletonChecked ? '✓' : '';
  hashSection.classList.toggle('dimmed', skeletonChecked);
  fileSection.classList.toggle('dimmed', skeletonChecked);
});

// ── Clear buttons ──
document.getElementById('clearHashes').addEventListener('click', (e) => {
  e.preventDefault();
  document.getElementById('hashes').value = '';
  document.getElementById('hashes').focus();
});
document.getElementById('clearFiles').addEventListener('click', (e) => {
  e.preventDefault();
  document.getElementById('files').value = '';
  document.getElementById('files').focus();
});

// ── Restore saved host/port ──
chrome.storage.local.get(['host', 'port'], (result) => {
  if (result.host) document.getElementById('host').value = result.host;
  if (result.port) document.getElementById('port').value = result.port;
});
['host', 'port'].forEach(id => {
  document.getElementById(id).addEventListener('change', () => {
    chrome.storage.local.set({
      host: document.getElementById('host').value,
      port: document.getElementById('port').value
    });
  });
});

// ── Status helpers ──
function setStatus(type, html) {
  const box     = document.getElementById('statusBox');
  const spinner = document.getElementById('spinner');
  const content = document.getElementById('statusContent');
  box.className = 'status-box ' + type;
  spinner.style.display = type === 'loading' ? '' : 'none';
  content.innerHTML = html;
}
function clearStatus() {
  const box = document.getElementById('statusBox');
  box.className = 'status-box';
}

// ── Fetch & Paste ──
document.getElementById('fetchBtn').addEventListener('click', doFetch);

// Allow Enter in hashes/files textarea to trigger fetch when Ctrl/Cmd held
document.getElementById('hashes').addEventListener('keydown', (e) => {
  if (e.key === 'Enter' && (e.ctrlKey || e.metaKey)) { e.preventDefault(); doFetch(); }
});
document.getElementById('files').addEventListener('keydown', (e) => {
  if (e.key === 'Enter') { e.preventDefault(); doFetch(); }
});

async function doFetch() {
  const hashesText = document.getElementById('hashes').value;
  const filesText  = document.getElementById('files').value;
  let host = document.getElementById('host').value.trim() || '127.0.0.1';
  let port = document.getElementById('port').value.trim() || '7890';

  const params = new URLSearchParams();

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
  btn.disabled = true;
  setStatus('loading', 'Fetching from daemon…');

  try {
    const response = await chrome.runtime.sendMessage({
      action: 'fetchAndPaste',
      host,
      port,
      params: params.toString()
    });

    if (response.success) {
      const items = response.summaries.map(s =>
        `<div class="status-item">✓ <span>${escapeHtml(s)}</span></div>`
      ).join('');
      setStatus('success',
        `<div class="status-title">Copied to clipboard</div>` +
        `<div class="status-items">${items}</div>`
      );
    } else {
      setStatus('error', escapeHtml(response.error));
    }
  } catch (err) {
    setStatus('error', 'Communication error: ' + escapeHtml(err.message));
  } finally {
    btn.disabled = false;
  }
}

function escapeHtml(str) {
  return String(str)
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;');
}
