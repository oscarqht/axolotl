(() => {
    const post = window.ipc.postMessage.bind(window.ipc);
    const nonce = '__NONCE__';
    const send = value => post(nonce + ':' + value);
    // Read favicons in the page's own origin and send only a small raster to chrome.
    const favicon = () => {
        if (window.top !== window || !/^https?:$/.test(location.protocol)) return;
        const icon = document.querySelector('link[rel~="icon"]');
        const fallback = location.origin + '/favicon.ico';
        let source = fallback;
        try {
            const url = new URL(icon?.href || fallback, location.href);
            if ((/^https?:$/.test(url.protocol) && url.origin === location.origin) || /^data:image\//i.test(url.href)) source = url.href;
        } catch {}
        const image = new Image();
        // Ordinary image mode avoids unnecessary CORS failures when a same-origin
        // favicon redirects. Canvas origin checks still reject unreadable pixels;
        // declared cross-origin icons use the page-origin fallback instead.
        const failed = () => { if (image.src !== fallback) image.src = fallback; };
        image.onload = () => { try {
            const canvas = document.createElement('canvas'); canvas.width = canvas.height = 32;
            canvas.getContext('2d').drawImage(image, 0, 0, 32, 32);
            send('favicon:' + canvas.toDataURL('image/png').split(',')[1]);
        } catch { failed(); } };
        image.onerror = failed;
        image.src = source;
    };
    window.addEventListener('load', favicon);
    window.addEventListener('pointerdown' , event => {
        if (event.isTrusted) send('focus');
    }, true);
    window.addEventListener('focus', event => {
        if (event.isTrusted) send('focus');
    });
    window.addEventListener('keydown', event => {
        if (!event.isTrusted) return;
        if (event.key === 'Escape') {
            send('escape');
            return;
        }
        if (event.key.toLowerCase() === 't' && event.shiftKey && !event.altKey && (__LINK_MODIFIER__)) {
            event.preventDefault(); event.stopImmediatePropagation(); send('new-space');
            return;
        }
        if (!(event.metaKey || event.ctrlKey) || event.altKey || event.shiftKey) return;
        if (event.key === ',' && !(__LINK_MODIFIER__)) return;
        const command = {l:'address', t:'new-tab', w:'close-tab', r:'reload', ',':'settings'}[event.key.toLowerCase()];
        if (command) { event.preventDefault(); event.stopImmediatePropagation(); send(command); }
    }, true);
    window.addEventListener('click', event => {
        if (!event.isTrusted || event.defaultPrevented || event.button !== 0 || event.shiftKey) return;
        const popup = event.altKey && !event.metaKey && !event.ctrlKey;
        const sibling = !event.altKey && (__LINK_MODIFIER__);
        if (!popup && !sibling) return;
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
        send((popup ? 'popup-link:' : 'open-link:') + url.href);
    }, true);
__GESTURES__
})();
