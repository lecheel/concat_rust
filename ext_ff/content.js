// content.js – not required for the basic flow, but can be used for additional features
chrome.runtime.onMessage.addListener((msg, sender, sendResponse) => {
  if (msg.action === "pasteFromBackground") {
    pasteFromClipboard();
    sendResponse({ done: true });
  }
});
