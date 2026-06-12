// === ext/background.js ===
// CHANGES: Added fetchRepos message handler

// Open side panel when the extension action icon is clicked
chrome.sidePanel.setPanelBehavior({ openPanelOnActionClick: true });

chrome.runtime.onInstalled.addListener(() => {
  chrome.contextMenus.create({
    id: "fetchHash",
    title: "Fetch this hash from concat_rust",
    contexts: ["selection"]
  });
});

chrome.contextMenus.onClicked.addListener(async (info, tab) => {
  if (info.menuItemId === "fetchHash") {
    const hash = info.selectionText.trim();
    if (hash) {
      chrome.storage.local.get(['host', 'port'], async (result) => {
        const host = result.host || '127.0.0.1';
        const port = result.port || '7890';
        const paramsStr = `hash=${encodeURIComponent(hash)}`;
        try {
          await fetchAndPaste(host, port, paramsStr);
        } catch (err) {
          console.error("Context menu operation failed:", err);
        }
      });
    }
  }
});

chrome.runtime.onMessage.addListener((message, sender, sendResponse) => {
  if (message.action === 'fetchAndPaste') {
    fetchAndPaste(message.host, message.port, message.params)
      .then(result => sendResponse(result))
      .catch(err => sendResponse({ success: false, error: err.message }));
    return true;
  }

  if (message.action === 'fetchRepos') {
    const host = message.host || '127.0.0.1';
    const port = message.port || '7890';
    const url = `http://${host}:${port}/repos`;
    fetch(url)
      .then(resp => {
        if (!resp.ok) throw new Error(`HTTP ${resp.status}`);
        return resp.json();
      })
      .then(data => sendResponse({ success: true, data }))
      .catch(err => sendResponse({ success: false, error: err.message }));
    return true;
  }
});

async function fetchAndPaste(host, port, paramsStr) {
  const baseUrl = `http://${host}:${port}`;
  const params = new URLSearchParams(paramsStr);
  let clipboardContent = '';
  const summaries = [];

  async function fetchUrl(url, label) {
    const response = await fetch(url);
    if (!response.ok) {
      throw new Error(`HTTP ${response.status} for ${label}`);
    }
    const text = await response.text();
    if (clipboardContent) clipboardContent += '\n\n';
    clipboardContent += text;
    summaries.push(label);
  }

  try {
    if (params.has('skeleton')) {
      const repo = params.get('repo') || '';
      const url = repo ? `${baseUrl}/skeleton?repo=${encodeURIComponent(repo)}` : `${baseUrl}/skeleton`;
      await fetchUrl(url, 'SKELETON (full output)');
    } else {
      const hashes = params.getAll('hash');
      if (hashes.length > 0) {
        const hashQuery = hashes.join('+');
        const resp = await fetch(`${baseUrl}/${hashQuery}`);
        if (!resp.ok) {
          throw new Error(`Hashes query failed (HTTP ${resp.status})`);
        }
        const text = await resp.text();
        if (clipboardContent) clipboardContent += '\n\n';
        clipboardContent += text;

        const lines = text.split('\n');
        const foundFiles = new Set();
        for (const line of lines) {
          if (line.startsWith('//--+ file:///')) {
            foundFiles.add(line.replace('//--+ file:///', ''));
          }
        }

        if (foundFiles.size > 0) {
          const filesList = Array.from(foundFiles).join(', ');
          summaries.push(`${hashes.length} block(s) -> [${filesList}]`);
        } else {
          summaries.push(`${hashes.length} block(s)`);
        }
      }

      for (const filepath of params.getAll('file')) {
        const safePath = filepath.split('/').map(encodeURIComponent).join('/');
        await fetchUrl(`${baseUrl}/file/${safePath}`, filepath);
      }
    }

    if (!clipboardContent) throw new Error('No content retrieved');

    const [tab] = await chrome.tabs.query({ active: true, currentWindow: true });
    if (tab?.id) {
      await chrome.scripting.executeScript({
        target: { tabId: tab.id },
        func: pasteAndCopyToClipboardInPage,
        args: [clipboardContent]
      });
    }
    return { success: true, summaries };
  } catch (err) {
    return { success: false, error: err.message };
  }
}

// In background.js
function pasteViaClipboardEvent(target, text) {
  // Build a DataTransfer with the text attached
  const dt = new DataTransfer();
  dt.setData('text/plain', text);

  const pasteEvent = new ClipboardEvent('paste', {
    bubbles: true,
    cancelable: true,
    clipboardData: dt,
  });

  // Fire it — the LLM UI's own paste handler picks it up
  // exactly as if the user pressed Ctrl+V / Shift+Insert
  target.dispatchEvent(pasteEvent);
}

function pasteAndCopyToClipboardInPage(textToPaste) {
  // 1. Copy to system clipboard (so user can paste normally elsewhere)
  navigator.clipboard.writeText(textToPaste).catch(err => {
    console.warn('System clipboard write failed:', err);
  });

  // 2. Find best editable target
  let target = document.activeElement;
  let isEditable = target && (
    target.isContentEditable ||
    target.tagName === 'TEXTAREA' ||
    (target.tagName === 'INPUT' && !['button','checkbox','radio','submit','hidden'].includes(target.type))
  );

  if (!isEditable) {
    target = findBestEditable();
  }

  if (!target) {
    console.warn('No editable field found. Content copied to clipboard only.');
    return;
  }

  target.focus();

  // 3. Handle standard form fields (direct value assignment)
  if (target.tagName === 'TEXTAREA' || target.tagName === 'INPUT') {
    const start = target.selectionStart;
    const end   = target.selectionEnd;
    const value = target.value;
    target.value = value.slice(0, start) + textToPaste + value.slice(end);
    target.dispatchEvent(new Event('input', { bubbles: true }));
    target.setSelectionRange(start + textToPaste.length, start + textToPaste.length);

  // 4. Handle contenteditable (LLM inputs) – with real ClipboardEvent
  } else if (target.isContentEditable) {
    const dt = new DataTransfer();
    dt.setData('text/plain', textToPaste);
    const pasteEvent = new ClipboardEvent('paste', {
      bubbles: true,
      cancelable: true,
      clipboardData: dt,
    });
    const handled = target.dispatchEvent(pasteEvent);

    // If the page didn't prevent default, fall back to execCommand (rare)
    if (!pasteEvent.defaultPrevented) {
      document.execCommand('insertText', false, textToPaste);
    }

    // Trigger input event for any remaining listeners
    target.dispatchEvent(new Event('input', { bubbles: true }));
  }
}

function findBestEditable() {
  const candidates = [];

  const isVisible = (el) => {
    if (el.offsetWidth === 0 && el.offsetHeight === 0) return false;
    const s = getComputedStyle(el);
    return s.display !== 'none' && s.visibility !== 'hidden' && parseFloat(s.opacity) > 0;
  };

  document.querySelectorAll('textarea').forEach(el => {
    if (isVisible(el)) candidates.push({ el, priority: 3 });
  });

  document.querySelectorAll('input[type="text"], input[type="search"], input:not([type])').forEach(el => {
    if (isVisible(el)) candidates.push({ el, priority: 2 });
  });

  document.querySelectorAll('[contenteditable="true"], [contenteditable=""]').forEach(el => {
    if (isVisible(el)) candidates.push({ el, priority: 1 });
  });

  if (candidates.length === 0) return null;

  candidates.sort((a, b) => {
    if (b.priority !== a.priority) return b.priority - a.priority;
    const aArea = a.el.offsetWidth * a.el.offsetHeight;
    const bArea = b.el.offsetWidth * b.el.offsetHeight;
    return bArea - aArea;
  });

  return candidates[0].el;
}
