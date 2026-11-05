(() => {
    const id = 'axolotl-copy-url-toast';
    document.getElementById(id)?.remove();
    const toast = document.createElement('div');
    toast.id = id;
    toast.setAttribute('role', 'status');
    toast.setAttribute('aria-live', 'polite');
    toast.setAttribute('aria-atomic', 'true');
    // Inline styles also work on pages that disallow injected stylesheets.
    // Reset page styling and keep the toast out of hit testing and tab order.
    const styles = {
        all: 'initial', position: 'fixed', top: '16px', right: '16px',
        bottom: 'auto', left: 'auto', display: 'block', margin: '0',
        padding: '8px 12px', width: 'max-content', height: 'auto',
        'max-width': 'calc(100vw - 32px)', 'box-sizing': 'border-box',
        background: '#111', color: '#fff', 'border-radius': '6px',
        border: '0', 'box-shadow': '0 2px 8px #0003',
        font: '500 12px/18px -apple-system, BlinkMacSystemFont, sans-serif',
        'z-index': '2147483647', 'pointer-events': 'none',
        opacity: '1', visibility: 'visible', transform: 'none',
        transition: 'none', animation: 'none'
    };
    for (const [property, value] of Object.entries(styles)) {
        toast.style.setProperty(property, value, 'important');
    }
    toast.attachShadow({ mode: 'open' }).textContent = 'URL copied';
    (document.fullscreenElement || document.documentElement).append(toast);
    // Each timeout owns its node so an older copy cannot dismiss a newer toast.
    setTimeout(() => toast.remove(), 1600);
})();
