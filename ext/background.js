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
      await fetchUrl(`${baseUrl}/skeleton`, 'SKELETON (full output)');
    } else {
      for (const hash of params.getAll('hash')) {
        const resp = await fetch(`${baseUrl}/${hash}`);
        if (!resp.ok) throw new Error(`Hash ${hash} -> HTTP ${resp.status}`);
        const text = await resp.text();
        const firstLine = text.split('\n')[0] || '';
        const filename = firstLine.startsWith('// File: ') ? firstLine.slice(9) : 'unknown';
        if (clipboardContent) clipboardContent += '\n\n';
        clipboardContent += text;
        summaries.push(`${hash.slice(0, 8)}… (${filename})`);
      }
      for (const filepath of params.getAll('file')) {
        await fetchUrl(`${baseUrl}/file/${encodeURIComponent(filepath)}`, filepath);
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

function pasteAndCopyToClipboardInPage(textToPaste) {
  navigator.clipboard.writeText(textToPaste).catch(err => {
    console.warn('System clipboard write failed:', err);
  });

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

  if (target.tagName === 'TEXTAREA' || target.tagName === 'INPUT') {
    const start = target.selectionStart;
    const end   = target.selectionEnd;
    const value = target.value;
    target.value = value.slice(0, start) + textToPaste + value.slice(end);
    target.dispatchEvent(new Event('input', { bubbles: true }));
    target.setSelectionRange(start + textToPaste.length, start + textToPaste.length);
  } else if (target.isContentEditable) {
    target.focus();
    const selection = window.getSelection();
    if (selection.rangeCount) {
      const range = selection.getRangeAt(0);
      range.deleteContents();
      range.insertNode(document.createTextNode(textToPaste));
      range.collapse(false);
      selection.removeAllRanges();
      selection.addRange(range);
    } else {
      target.innerText += textToPaste;
    }
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

  // Textareas — highest priority (big area = likely chat/compose)
  document.querySelectorAll('textarea').forEach(el => {
    if (isVisible(el)) candidates.push({ el, priority: 3 });
  });

  // Text/search inputs
  document.querySelectorAll('input[type="text"], input[type="search"], input:not([type])').forEach(el => {
    if (isVisible(el)) candidates.push({ el, priority: 2 });
  });

  // Contenteditable divs (many chat apps use these)
  document.querySelectorAll('[contenteditable="true"], [contenteditable=""]').forEach(el => {
    if (isVisible(el)) candidates.push({ el, priority: 1 });
  });

  if (candidates.length === 0) return null;

  // Sort: priority desc, then area desc (larger = more likely the main input)
  candidates.sort((a, b) => {
    if (b.priority !== a.priority) return b.priority - a.priority;
    const aArea = a.el.offsetWidth * a.el.offsetHeight;
    const bArea = b.el.offsetWidth * b.el.offsetHeight;
    return bArea - aArea;
  });

  return candidates[0].el;
}
