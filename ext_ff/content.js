chrome.runtime.onMessage.addListener((msg, sender, sendResponse) => {
  if (msg.action === 'pasteFromBackground') {
    pasteFromClipboard();
    sendResponse({ done: true });
  }
});
