(() => {
    const post = window.ipc.postMessage.bind(window.ipc);
    const nonce = '__NONCE__';
    const send = value => post(nonce + ':' + value);
    // Read favicons in the page's own origin and send only a small raster to chrome.
    const favicon = () => {
        if (window.top !== window) return;
        const icon = document.querySelector('link[rel~="icon"]');
        const image = new Image();
        image.crossOrigin = 'anonymous';
        image.onload = () => { try {
            const canvas = document.createElement('canvas'); canvas.width = canvas.height = 32;
            canvas.getContext('2d').drawImage(image, 0, 0, 32, 32);
            send('favicon:' + canvas.toDataURL('image/png').split(',')[1]);
        } catch {} };
        image.onerror = () => { if (icon && image.src !== location.origin + '/favicon.ico') image.src = location.origin + '/favicon.ico'; };
        if (/^https?:$/.test(location.protocol)) image.src = icon?.href || location.origin + '/favicon.ico';
    };
    window.addEventListener('load', favicon);
    window.addEventListener('pointerdown' , event => {
        if (event.isTrusted) send('focus');
    }, true);
    window.addEventListener('focus', event => {
        if (event.isTrusted) send('focus');
    });
    window.addEventListener('keydown', event => {
        if (event.isTrusted && event.key === 'Escape') { send('escape'); return; }
        if (!event.isTrusted || !(event.metaKey || event.ctrlKey) || event.altKey || event.shiftKey) return;
        if (event.key === ',' && !(__LINK_MODIFIER__)) return;
        const command = {l:'address', t:'new-tab', w:'close-tab', r:'reload', ',':'settings'}[event.key.toLowerCase()];
        if (command) { event.preventDefault(); event.stopImmediatePropagation(); send(command); }
    }, true);
    window.addEventListener('click', event => {
        if (!event.isTrusted || event.defaultPrevented || event.button !== 0 ||
            !(__LINK_MODIFIER__) || event.altKey || event.shiftKey) return;
        const link = event.composedPath().find(node =>
            node instanceof Element && node.matches('a[href], area[href]'));
        if (!link || link.hasAttribute('download')) return;
        let url;
        try { url = new URL(link.getAttribute('href'), link.baseURI); }
        catch { return; }
        if (!['http:', 'https:'].includes(url.protocol) && url.href !== 'about:blank') return;
        // Cancel both same-frame navigation and target=_blank's native popup.
        event.preventDefault();
        event.stopImmediatePropagation();
        send('open-link:' + url.href);
    }, true);
__GESTURES__
})();
