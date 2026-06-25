chrome.runtime.onMessage.addListener((msg, sender, sendResponse) => {
  if (msg.action === 'pasteFromBackground') {
    pasteFromClipboard();
    sendResponse({ done: true });
  }
});

// ─── Detect copy from ds-floating-container ───

let containerDetected = false;

function findFloatingContainer() {
  const all = document.querySelectorAll('[class*="ds-floating-container"]');
  for (const el of all) {
    const style = getComputedStyle(el);
    if (style.zIndex && style.zIndex !== 'auto' && parseInt(style.zIndex, 10) > 0) {
      return el;
    }
  }
  return null;
}

function attachCopyListener(container) {
  if (containerDetected) return;
  containerDetected = true;

  // Listen for copy events that originate inside the container
  document.addEventListener('copy', function (e) {
    if (container.contains(e.target)) {
      const plainText = e.clipboardData ? e.clipboardData.getData('text/plain') : '';
      if (plainText) {
        const preview = plainText.length > 200 ? plainText.substring(0, 200) + '…' : plainText;
        chrome.runtime.sendMessage({
          action: 'copyDetected',
          text: preview,
          fullText: plainText
        });
      }
    }
  });

  // Optional: also detect clicks on likely copy buttons for extra feedback
  container.addEventListener('click', function (e) {
    const btn = e.target.closest('button[aria-label*="copy" i], .copy-btn, [data-action="copy"]');
    if (btn) {
      chrome.runtime.sendMessage({
        action: 'copyButtonClicked',
        buttonText: btn.textContent || 'Copy'
      });
    }
  });
}

function observeContainer() {
  const existing = findFloatingContainer();
  if (existing) {
    attachCopyListener(existing);
    return;
  }

  const observer = new MutationObserver(() => {
    const newContainer = findFloatingContainer();
    if (newContainer && !containerDetected) {
      attachCopyListener(newContainer);
      observer.disconnect(); // stop observing once found
    }
  });
  observer.observe(document.body, { childList: true, subtree: true });
}

// Run after page ready
if (document.readyState === 'loading') {
  document.addEventListener('DOMContentLoaded', observeContainer);
} else {
  observeContainer();
}
