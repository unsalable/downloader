// Fill a static page from _locales.
//
// Both pages are plain HTML with the English text living in messages.json
// rather than in the markup, so a Turkish popup is a translation rather than a
// second page that has to be kept in step with the first.

function localize(root = document) {
  for (const node of root.querySelectorAll('[data-i18n]')) {
    const text = chrome.i18n.getMessage(node.dataset.i18n);
    if (text) node.textContent = text;
  }
  for (const node of root.querySelectorAll('[data-i18n-title]')) {
    const text = chrome.i18n.getMessage(node.dataset.i18nTitle);
    if (text) node.title = text;
  }
  // The document language is what decides hyphenation and the quotation marks a
  // browser draws, and Chrome does not set it for an extension page.
  document.documentElement.lang = chrome.i18n.getUILanguage();
}

localize();
