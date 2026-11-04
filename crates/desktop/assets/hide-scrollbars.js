(() => {
    // Hide only native scrollbar painting, never overflow or input events.
    const css = `
        * { scrollbar-width: none !important; }
        ::-webkit-scrollbar {
            display: none !important;
            width: 0 !important;
            height: 0 !important;
        }
    `;

    // Constructed stylesheets also work on pages whose CSP forbids inline
    // <style> elements, and do not require a document element at document-start.
    if (typeof CSSStyleSheet !== 'undefined' &&
        'replaceSync' in CSSStyleSheet.prototype &&
        'adoptedStyleSheets' in document) {
        const sheet = new CSSStyleSheet();
        sheet.replaceSync(css);
        document.adoptedStyleSheets = [...document.adoptedStyleSheets, sheet];
        return;
    }

    // Older WebKit versions need a style element and may not have a root yet.
    const style = document.createElement('style');
    style.textContent = css;
    const install = () => {
        if (!document.documentElement) return false;
        document.documentElement.appendChild(style);
        return true;
    };
    if (!install()) {
        const observer = new MutationObserver(() => {
            if (install()) observer.disconnect();
        });
        observer.observe(document, { childList: true });
    }
})();
