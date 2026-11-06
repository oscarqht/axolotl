(() => {
    if (window.__axolotlOpenFind) { window.__axolotlOpenFind(); return; }
    if (!document.body) return;
    const host = document.createElement('div');
    host.id = 'axolotl-page-find';
    host.style.cssText = 'all:initial!important;position:fixed!important;top:8px!important;right:8px!important;z-index:2147483647!important;max-width:calc(100vw - 16px)!important';
    const shadow = host.attachShadow({mode:'open'});
    shadow.innerHTML = `<style>
        :host { color-scheme:light dark; }
        form { display:flex;align-items:center;gap:6px;margin:0;padding:8px;border:1px solid #8886;
            border-radius:8px;background:Canvas;color:CanvasText;box-shadow:0 4px 18px #0003;font:13px system-ui; }
        input { width:170px;min-width:50px;flex:1;border:0;outline:0;background:transparent;color:inherit;font:inherit; }
        button { border:0;border-radius:4px;background:transparent;color:inherit;padding:3px 6px;cursor:pointer; }
        button:hover { background:#8883; } button:focus-visible { outline:2px solid Highlight; }
        output { white-space:nowrap;font-variant-numeric:tabular-nums; }
    </style><form role="search" aria-label="Find in page">
        <input type="text" aria-label="Find in page" placeholder="Find in page" autocomplete="off" spellcheck="false">
        <output aria-live="polite">0/0</output>
        <button type="button" title="Previous match (Shift+Enter)" aria-label="Previous match">↑</button>
        <button type="button" title="Next match (Enter)" aria-label="Next match">↓</button>
        <button type="button" title="Close (Escape)" aria-label="Close find">×</button>
    </form>`;
    const input = shadow.querySelector('input'), output = shadow.querySelector('output');
    const buttons = shadow.querySelectorAll('button');
    let ranges = [], index = -1, previousFocus = null, timer = null, open = false;
    const allName = 'axolotl-find-matches', activeName = 'axolotl-find-active';
    const style = document.createElement('style');
    style.textContent = `::highlight(${allName}) { background:#ffe082;color:#171717; }
        ::highlight(${activeName}) { background:#ff9632;color:#171717; }`;
    // Constructed stylesheets also work on sites forbidding inline style tags.
    const sheetFor = text => {
        try { const sheet = new CSSStyleSheet(); sheet.replaceSync(text); return sheet; }
        catch { return null; }
    };
    const chromeStyle = shadow.querySelector('style');
    const chromeSheet = sheetFor(chromeStyle.textContent);
    if (chromeSheet) { shadow.adoptedStyleSheets = [chromeSheet]; chromeStyle.remove(); }
    const matchSheet = sheetFor(style.textContent);
    const highlights = typeof Highlight !== 'undefined' && CSS.highlights;
    const clear = () => {
        if (highlights) { CSS.highlights.delete(allName); CSS.highlights.delete(activeName); }
    };
    const paint = (scroll = false) => {
        clear();
        output.textContent = `${index + 1}/${ranges.length}`;
        buttons[0].disabled = buttons[1].disabled = ranges.length === 0;
        if (highlights) {
            const matches = new Highlight();
            for (const range of ranges) matches.add(range);
            CSS.highlights.set(allName, matches);
            if (index >= 0) CSS.highlights.set(activeName, new Highlight(ranges[index]));
        }
        const range = ranges[index];
        if (!range) return;
        if (!highlights) {
            const selection = window.getSelection(); selection.removeAllRanges(); selection.addRange(range);
        }
        if (scroll) {
            // Scroll the match itself, not just the start of a long paragraph.
            for (let element = range.startContainer.parentElement; element; element = element.parentElement) {
                const rect = range.getBoundingClientRect(), bounds = element.getBoundingClientRect();
                const css = getComputedStyle(element);
                if (/(auto|scroll|hidden)/.test(css.overflowY) && (rect.top < bounds.top || rect.bottom > bounds.bottom))
                    element.scrollTop += rect.top - bounds.top - element.clientHeight / 2;
                if (/(auto|scroll|hidden)/.test(css.overflowX) && (rect.left < bounds.left || rect.right > bounds.right))
                    element.scrollLeft += rect.left - bounds.left - element.clientWidth / 2;
            }
            const rect = range.getBoundingClientRect();
            if (rect.top < 0 || rect.bottom > innerHeight || rect.left < 0 || rect.right > innerWidth)
                window.scrollBy({top:rect.top - innerHeight / 2, left:rect.left < 0 || rect.right > innerWidth ? rect.left - innerWidth / 2 : 0});
        }
    };
    const search = () => {
        clearTimeout(timer); timer = null;
        ranges = []; index = -1;
        const query = input.value;
        if (!query) { paint(); return; }
        // Group adjacent inline text, but never join unrelated blocks or hidden
        // content. Ranges highlight without rewriting the site's DOM or editors.
        const groups = [];
        let group = null;
        const walker = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT);
        for (let node; (node = walker.nextNode());) {
            const element = node.parentElement;
            if (!element || host.contains(element) || element.closest('script,style,noscript,textarea,input,select,[hidden],[aria-hidden="true"]')) continue;
            const range = document.createRange(); range.selectNodeContents(node);
            const computed = getComputedStyle(element);
            if (!range.getClientRects().length || computed.visibility === 'hidden' || computed.visibility === 'collapse') continue;
            let block = element;
            while (block.parentElement && getComputedStyle(block).display === 'inline') block = block.parentElement;
            if (!group || group.block !== block) { group = {block, text:'', nodes:[]}; groups.push(group); }
            group.nodes.push({node, start:group.text.length}); group.text += node.data;
        }
        // Escaped Unicode regexp preserves original UTF-16 offsets even for
        // case mappings whose lowercase spelling has a different length.
        const escaped = Array.from(query, char => '\\u{' + char.codePointAt(0).toString(16) + '}').join('');
        const pattern = new RegExp(escaped, 'giu');
        for (const group of groups) {
            for (const match of group.text.matchAll(pattern)) {
                const start = match.index, end = start + match[0].length;
                const first = group.nodes.findLast(entry => entry.start <= start);
                const last = group.nodes.findLast(entry => entry.start < end);
                const range = document.createRange();
                range.setStart(first.node, start - first.start); range.setEnd(last.node, end - last.start);
                ranges.push(range);
            }
        }
        index = ranges.length ? 0 : -1; paint(true);
    };
    const step = direction => {
        if (timer) { search(); return; }
        // Refresh disconnected ranges after SPA edits before navigating.
        if (ranges.some(range => !range.startContainer.isConnected)) search();
        if (!ranges.length) return;
        index = (index + direction + ranges.length) % ranges.length; paint(true);
    };
    const observer = new MutationObserver(records => {
        if (open && records.some(record => !host.contains(record.target))) {
            clearTimeout(timer); timer = setTimeout(search, 120);
        }
    });
    const close = () => {
        if (!open) return false;
        open = false; clearTimeout(timer); observer.disconnect(); clear();
        if (!highlights) window.getSelection()?.removeAllRanges();
        ranges = []; index = -1; host.remove(); style.remove();
        if (matchSheet) document.adoptedStyleSheets = document.adoptedStyleSheets.filter(sheet => sheet !== matchSheet);
        if (previousFocus?.isConnected) previousFocus.focus({preventScroll:true});
        else window.focus();
        return true;
    };
    const show = () => {
        if (!open) {
            previousFocus = document.activeElement;
            document.documentElement.append(host);
            if (matchSheet) document.adoptedStyleSheets = [...document.adoptedStyleSheets, matchSheet];
            else document.documentElement.append(style);
            open = true;
            observer.observe(document.body, {subtree:true, childList:true, characterData:true, attributes:true, attributeFilter:['hidden','style','class']});
            search();
        }
        input.focus(); input.select();
    };
    input.addEventListener('input', () => { clearTimeout(timer); timer = setTimeout(search, 80); });
    shadow.querySelector('form').addEventListener('submit', event => event.preventDefault());
    const findKey = event => {
        if (!open || event.isComposing) return false;
        if (event.key === 'Enter' && event.composedPath().includes(host)) {
            event.preventDefault(); event.stopImmediatePropagation();
            if (timer) search(); else step(event.shiftKey ? -1 : 1);
            return true;
        }
        return false;
    };
    // The initialization bridge runs before page key handlers, so websites
    // cannot steal Enter from the focused find field.
    window.__axolotlFindKey = findKey;
    shadow.addEventListener('keydown', findKey);
    buttons[0].onclick = () => step(-1); buttons[1].onclick = () => step(1); buttons[2].onclick = close;
    window.__axolotlOpenFind = show;
    window.__axolotlCloseFind = close;
    show();
})();
