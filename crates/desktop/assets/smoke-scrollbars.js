(() => {
    const root = document.scrollingElement;
    const oldPosition = [scrollX, scrollY];
    const fixture = document.createElement('div');
    fixture.style.cssText = 'position:relative;width:200vw;height:200vh';
    fixture.innerHTML = '<div><div></div></div>';
    const nested = fixture.firstElementChild;
    nested.style.cssText = 'width:120px;height:120px;overflow:scroll;scroll-behavior:auto';
    nested.firstElementChild.style.cssText = 'width:480px;height:480px';
    document.body.appendChild(fixture);
    const hidden = element => {
        const normal = getComputedStyle(element);
        const bar = getComputedStyle(element, '::-webkit-scrollbar');
        return normal.getPropertyValue('scrollbar-width') === 'none' ||
            (bar.display === 'none' && bar.width === '0px' && bar.height === '0px');
    };
    try {
        root.scrollLeft = 40;
        root.scrollTop = 40;
        nested.scrollLeft = 30;
        nested.scrollTop = 30;
        return {
            rootHidden: hidden(root),
            nestedHidden: hidden(nested),
            rootScrolls: root.scrollLeft > 0 && root.scrollTop > 0,
            nestedScrolls: nested.scrollLeft === 30 && nested.scrollTop === 30,
            overflowPreserved: getComputedStyle(nested).overflow === 'scroll',
            noNestedGutter: nested.offsetWidth === nested.clientWidth &&
                nested.offsetHeight === nested.clientHeight,
        };
    } finally {
        fixture.remove();
        scrollTo(...oldPosition);
    }
})();
