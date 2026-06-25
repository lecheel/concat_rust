// Firefox: Open sidebar when the extension action icon is clicked
chrome.action.onClicked.addListener((tab) => {
  if (typeof browser !== 'undefined' && browser.sidebarAction) {
    browser.sidebarAction.open();
  }
});

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
      chrome.storage.local.get(['host', 'port', 'activeRepo'], async (result) => {
        const host = result.host || '127.0.0.1';
        const port = result.port || '7890';
        const activeRepo = result.activeRepo || '';
        let paramsStr = `hash=${encodeURIComponent(hash)}`;
        if (activeRepo) paramsStr += `&repo=${encodeURIComponent(activeRepo)}`;
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
    fetchAndPaste(message.host, message.port, message.params, {
      pasteAsFile: message.pasteAsFile,
      filename: message.filename
    })
      .then(result => sendResponse(result))
      .catch(err => sendResponse({ success: false, error: err.message }));
    return true;
  }

  if (message.action === 'fetchRepos') {
    const host = message.host || '127.0.0.1';
    const port = message.port || '7890';
    fetch(`http://${host}:${port}/repos`)
      .then(resp => { if (!resp.ok) throw new Error(`HTTP ${resp.status}`); return resp.json(); })
      .then(data => sendResponse({ success: true, data }))
      .catch(err => sendResponse({ success: false, error: err.message }));
    return true;
  }

  if (message.action === 'fetchActiveRepo') {
    const host = message.host || '127.0.0.1';
    const port = message.port || '7890';
    fetch(`http://${host}:${port}/active`)
      .then(resp => { if (!resp.ok) throw new Error(`HTTP ${resp.status}`); return resp.text(); })
      .then(text => sendResponse({ success: true, repo: text.trim() }))
      .catch(err => sendResponse({ success: false, error: err.message }));
    return true;
  }

  // Copy detection from content script
  if (message.action === 'copyDetected' || message.action === 'copyButtonClicked') {
    const copyData = {
      text: message.text || 'Copy action detected',
      fullText: message.fullText || '',
      timestamp: Date.now()
    };

    // Store in local storage for sidepanel to pick up later
    chrome.storage.local.set({ lastCopy: copyData });

    // Notify the sidepanel if it's open
    chrome.runtime.sendMessage({
      target: 'sidepanel',
      action: 'showCopyInfo',
      data: copyData
    }).catch(() => {}); // ignore if no sidepanel listener

    // Optional: show a system notification
    try {
      chrome.notifications.create({
        type: 'basic',
        iconUrl: 'icon.png',
        title: '📋 Copy detected',
        message: copyData.text.length > 60 ? copyData.text.substring(0, 60) + '…' : copyData.text
      });
    } catch (e) {
      // notifications may fail if icon.png missing — ignore
    }

    sendResponse({ done: true });
    return true;
  }
});

async function fetchAndPaste(host, port, paramsStr, options = {}) {
  const baseUrl = `http://${host}:${port}`;
  const params = new URLSearchParams(paramsStr);
  let clipboardContent = '';
  const summaries = [];

  const repo = params.get('repo') || '';
  const repoQuery = repo ? `?repo=${encodeURIComponent(repo)}` : '';

  async function fetchUrl(url, label) {
    const response = await fetch(url);
    if (!response.ok) throw new Error(`HTTP ${response.status} for ${label}`);
    const text = await response.text();
    if (clipboardContent) clipboardContent += '\n\n';
    clipboardContent += text;
    summaries.push(label);
  }

  try {
    if (params.has('skeleton')) {
      const repoParam = params.get('repo') || '';
      const url = repoParam
        ? `${baseUrl}/skeleton?repo=${encodeURIComponent(repoParam)}`
        : `${baseUrl}/skeleton`;
      await fetchUrl(url, 'SKELETON (full output)');
    } else {
      const hashes = params.getAll('hash');
      if (hashes.length > 0) {
        const hashQuery = hashes.join('+');
        const resp = await fetch(`${baseUrl}/${hashQuery}${repoQuery}`);
        if (!resp.ok) throw new Error(`Hashes query failed (HTTP ${resp.status})`);
        const text = await resp.text();
        if (clipboardContent) clipboardContent += '\n\n';
        clipboardContent += text;

        const foundFiles = new Set();
        for (const line of text.split('\n')) {
          if (line.startsWith('//--+ file:///')) {
            foundFiles.add(line.replace('//--+ file:///', ''));
          }
        }
        summaries.push(foundFiles.size > 0
          ? `${hashes.length} block(s) -> [${Array.from(foundFiles).join(', ')}]`
          : `${hashes.length} block(s)`);
      }

      for (const filepath of params.getAll('file')) {
        const safePath = filepath.split('/').map(encodeURIComponent).join('/');
        await fetchUrl(`${baseUrl}/file/${safePath}${repoQuery}`, filepath);
      }
    }

    if (!clipboardContent) throw new Error('No content retrieved');

    const [tab] = await chrome.tabs.query({ active: true, currentWindow: true });
    if (tab?.id) {
      await chrome.scripting.executeScript({
        target: { tabId: tab.id },
        func: pasteAndCopyToClipboardInPage,
        args: [clipboardContent, { pasteAsFile: options.pasteAsFile, filename: options.filename }]
      });
    }

    // Fetch LOC info immediately after pasting
    let locInfo = null;
    try {
      const locResp = await fetch(`${baseUrl}/loc-info`);
      if (locResp.ok) {
        locInfo = await locResp.json();
      }
    } catch (e) {
      console.error("Failed to fetch loc-info", e);
    }

    return { success: true, summaries, text: clipboardContent, locInfo };
  } catch (err) {
    return { success: false, error: err.message };
  }
}

// ═══════════════════════════════════════════════════════════════════
// Self-contained function injected into the LLM page via
// chrome.scripting.executeScript. ALL helpers must be inline —
// the page context only sees this function's body.
// ═══════════════════════════════════════════════════════════════════
function pasteAndCopyToClipboardInPage(textToPaste, options = {}) {

  // ── Logging ──
  const T = '%c[Concat Paste]%c', F = 'color:#6366f1;font-weight:bold', R = 'color:inherit';
  const log  = (...a) => console.log(T, F, R, ...a);
  const warn = (...a) => console.warn(T, F, R, ...a);
  const ok   = (...a) => console.log(T, F, 'color:#22c55e', ...a);
  const err  = (...a) => console.error(T, F, 'color:#ef4444', ...a);

  // ── isElementVisible ──
  function isVisible(el) {
    if (!el) return false;
    if (el.offsetWidth === 0 && el.offsetHeight === 0) return false;
    const s = getComputedStyle(el);
    return s.display !== 'none' && s.visibility !== 'hidden' && parseFloat(s.opacity) > 0;
  }

  // ── findLLMInput ──
  function findLLMInput() {
    const selectors = [
      // ChatGLM
      'textarea#chat-input',
      // ChatGPT
      '#prompt-textarea',
      'div#prompt-textarea[contenteditable="true"]',
      'div[contenteditable="true"][data-placeholder]',
      // Claude
      'div.ProseMirror[contenteditable="true"]',
      'div[contenteditable="true"][data-testid="composer-input"]',
      // Gemini
      'div.ql-editor[contenteditable="true"]',
      // Generic textarea chat inputs
      'textarea[placeholder*="message" i]',
      'textarea[placeholder*="send" i]',
      'textarea[placeholder*="chat" i]',
      'textarea[placeholder*="prompt" i]',
      'textarea[placeholder*="ask" i]',
      'textarea[aria-label*="message" i]',
      'textarea[aria-label*="chat" i]',
      'textarea[aria-label*="prompt" i]',
      'textarea[role="textbox"]',
      // Generic contenteditable chat inputs
      'div[contenteditable="true"][aria-label*="prompt" i]',
      'div[contenteditable="true"][class*="input"]',
      'div[contenteditable="true"][class*="composer"]',
      'div[contenteditable="true"][class*="chat-input"]',
      'div[contenteditable="true"][class*="textarea"]',
      'div[contenteditable="true"][role="textbox"]',
      'div[contenteditable="true"][aria-label*="message" i]',
      'div[contenteditable="true"][aria-label*="chat" i]',
      'div[contenteditable="true"][placeholder]',
      // Last resort
      'textarea:not([type])',
      'div[contenteditable="true"]',
    ];
    for (const sel of selectors) {
      try {
        for (const el of document.querySelectorAll(sel)) {
          if (isVisible(el)) {
            log('Found input via:', sel, el.tagName, el.id ? '#' + el.id : '');
            return el;
          }
        }
      } catch (_) {}
    }
    log('No LLM input found via selectors');
    return null;
  }

  // ── findBestEditable (fallback) ──
  function findBestEditable() {
    const c = [];
    document.querySelectorAll('textarea').forEach(el => { if (isVisible(el)) c.push({ el, p: 3 }); });
    document.querySelectorAll('input[type="text"],input[type="search"],input:not([type])').forEach(el => { if (isVisible(el)) c.push({ el, p: 2 }); });
    document.querySelectorAll('[contenteditable="true"],[contenteditable=""]').forEach(el => { if (isVisible(el)) c.push({ el, p: 1 }); });
    if (!c.length) return null;
    c.sort((a, b) => b.p - a.p || (b.el.offsetWidth * b.el.offsetHeight) - (a.el.offsetWidth * a.el.offsetHeight));
    log('Fallback editable:', c[0].el.tagName, c[0].el.id || '');
    return c[0].el;
  }

  // ── activateElement ──
  function activate(el) {
    el.focus();
    el.dispatchEvent(new MouseEvent('mousedown', { bubbles: true, cancelable: true }));
    el.dispatchEvent(new MouseEvent('mouseup', { bubbles: true, cancelable: true }));
    el.dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true }));
    el.dispatchEvent(new FocusEvent('focusin', { bubbles: true }));
    el.dispatchEvent(new FocusEvent('focus'));
    // Cursor to end
    const sel = window.getSelection();
    if (sel) {
      const rng = document.createRange();
      if (el.childNodes.length > 0) {
        const last = el.childNodes[el.childNodes.length - 1];
        if (last.nodeType === Node.TEXT_NODE) rng.setStart(last, last.textContent.length);
        else rng.setStartAfter(last);
      } else {
        rng.setStart(el, 0);
      }
      rng.collapse(true);
      sel.removeAllRanges();
      sel.addRange(rng);
    }
  }

  // ── insertIntoInput (textarea / input) ──
  function insertIntoInput(el, text) {
    const s = el.selectionStart || el.value.length;
    const e = el.selectionEnd || el.value.length;
    el.value = el.value.slice(0, s) + text + el.value.slice(e);
    el.dispatchEvent(new Event('input', { bubbles: true }));
    el.dispatchEvent(new Event('change', { bubbles: true }));
    el.setSelectionRange(s + text.length, s + text.length);
    ok('✅ Inserted into textarea at pos ' + s);
  }

  // ── insertIntoContentEditable ──
  function insertIntoContentEditable(el, text) {
    const probe = text.substring(0, 30);

    // Method 1: execCommand('insertText')
    try {
      el.focus();
      const sel = window.getSelection(), rng = document.createRange();
      if (el.childNodes.length > 0) {
        const last = el.childNodes[el.childNodes.length - 1];
        if (last.nodeType === Node.TEXT_NODE) rng.setStart(last, last.textContent.length);
        else rng.setStartAfter(last);
      } else rng.setStart(el, 0);
      rng.collapse(true); sel.removeAllRanges(); sel.addRange(rng);
      if (document.execCommand('insertText', false, text) && el.textContent.includes(probe)) {
        ok('✅ Method 1: execCommand'); return;
      }
    } catch (_) {}

    // Method 2: InputEvent 'insertText'
    try {
      el.focus();
      const evt = new InputEvent('beforeinput', { bubbles: true, cancelable: true, inputType: 'insertText', data: text });
      el.dispatchEvent(evt);
      if (evt.defaultPrevented) { el.dispatchEvent(new Event('input', { bubbles: true })); ok('✅ Method 2: beforeinput'); return; }
      document.execCommand('insertText', false, text);
      el.dispatchEvent(new Event('input', { bubbles: true }));
      if (el.textContent.includes(probe)) { ok('✅ Method 2: beforeinput+exec'); return; }
    } catch (_) {}

    // Method 3: ClipboardEvent('paste')
    try {
      el.focus();
      const dt = new DataTransfer(); dt.setData('text/plain', text);
      const evt = new ClipboardEvent('paste', { bubbles: true, cancelable: true, clipboardData: dt });
      el.dispatchEvent(evt);
      if (evt.defaultPrevented) { el.dispatchEvent(new Event('input', { bubbles: true })); ok('✅ Method 3: paste event'); return; }
      document.execCommand('insertText', false, text);
      el.dispatchEvent(new Event('input', { bubbles: true }));
      if (el.textContent.includes(probe)) { ok('✅ Method 3: paste+exec'); return; }
    } catch (_) {}

    // Method 4: Direct DOM mutation (last resort)
    try {
      el.focus();
      const node = document.createTextNode(text);
      const sel = window.getSelection();
      if (sel.rangeCount > 0) {
        const rng = sel.getRangeAt(0); rng.deleteContents(); rng.insertNode(node);
        rng.setStartAfter(node); rng.collapse(true); sel.removeAllRanges(); sel.addRange(rng);
      } else el.appendChild(node);
      el.dispatchEvent(new Event('input', { bubbles: true }));
      el.dispatchEvent(new Event('change', { bubbles: true }));
      warn('⚠️ Method 4: Direct DOM (may need space to commit)');
    } catch (e) {
      err('❌ All methods failed:', e);
      log('Content is in clipboard — use Ctrl+V manually');
    }
  }

  // ═══════════════════ MAIN ═══════════════════

  log('Paste requested —', textToPaste.length, 'bytes');
  navigator.clipboard.writeText(textToPaste).catch(() => {});

  // File upload
  if (options.pasteAsFile) {
    const fi = document.querySelector('input[type="file"]');
    if (fi) {
      try {
        const dt = new DataTransfer();
        dt.items.add(new File([textToPaste], options.filename || 'file.rs', { type: 'text/plain' }));
        fi.files = dt.files;
        fi.dispatchEvent(new Event('change', { bubbles: true }));
        ok('✅ Pasted as file'); return;
      } catch (e) { warn('File upload failed:', e); }
    }
  }

  // Find target
  let target = findLLMInput();
  if (!target) {
    const a = document.activeElement;
    if (a && (a.isContentEditable || a.tagName === 'TEXTAREA' || (a.tagName === 'INPUT' && !['button','checkbox','radio','submit','hidden'].includes(a.type)))) {
      target = a;
      log('Using active element:', a.tagName, a.id || '');
    }
  }
  if (!target) target = findBestEditable();
  if (!target) { err('❌ No editable field found — content in clipboard only'); return; }

  log('Target:', target.tagName, target.id ? '#' + target.id : '');
  activate(target);

  if (target.tagName === 'TEXTAREA' || target.tagName === 'INPUT') insertIntoInput(target, textToPaste);
  else if (target.isContentEditable) insertIntoContentEditable(target, textToPaste);

  ok('✅ Paste complete');
}
