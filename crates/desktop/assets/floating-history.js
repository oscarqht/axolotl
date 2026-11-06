let historyItems = [], historySelection = -1, historyInput = null, historyTab = null, historyQuery = null;
function renderHistory() {
  const list = document.querySelector('#history');
  const open = historyItems.length > 0;
  list.hidden = !open;
  document.body.classList.toggle('history', open);
  list.replaceChildren(...historyItems.map((item, index) => {
    const button = document.createElement('button');
    button.role = 'option'; button.id = `history-${index}`;
    button.setAttribute('aria-selected', String(index === historySelection));
    const title = document.createElement('strong'), url = document.createElement('span');
    title.textContent = item.title || item.url; url.textContent = item.url;
    button.append(title, url);
    button.onmousedown = e => e.preventDefault();
    button.onclick = () => send({type:'navigate',tab:historyTab,value:item.url});
    return button;
  }));
  if (historyInput) {
    historyInput.setAttribute('aria-expanded', String(open));
    if (historySelection >= 0) historyInput.setAttribute('aria-activedescendant', `history-${historySelection}`);
    else historyInput.removeAttribute('aria-activedescendant');
  }
  list.querySelector('[aria-selected=true]')?.scrollIntoView({block:'nearest'});
}
window.clearHistory = () => {
  historyQuery = null;
  historyItems = []; historySelection = -1; renderHistory();
  send({type:'history-open',value:false});
};
window.updateHistory = (query, items) => {
  if (!historyInput?.isConnected || historyQuery !== query || historyInput.value !== query) return;
  historyItems = items; historySelection = -1; renderHistory();
  send({type:'history-open',value:items.length > 0});
};
function bindHistory(input, tab) {
  historyInput = input; historyTab = tab;
  input.setAttribute('role','combobox'); input.setAttribute('aria-autocomplete','list');
  input.setAttribute('aria-controls','history'); input.setAttribute('aria-expanded','false');
  input.autocomplete='off'; input.spellcheck=false;
  input.oninput = () => {
    // Keep the current list and native bounds until replacement matches arrive.
    if (!input.value.trim()) window.clearHistory();
    else historyQuery = input.value;
    send({type:'history-query',tab,value:input.value});
  };
  input.onblur = () => window.clearHistory();
  window.clearHistory();
}
function historyKey(e, input, tab) {
  if (e.isComposing || !historyItems.length) return false;
  if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
    const n = historyItems.length;
    historySelection = historySelection < 0 ? (e.key === 'ArrowDown' ? 0 : n-1) : (historySelection + (e.key === 'ArrowDown' ? 1 : n-1))%n;
    renderHistory();
  } else if (e.key === 'Escape') { window.clearHistory(); }
  else if (e.key === 'Enter' && historySelection >= 0) { send({type:'navigate',tab,value:historyItems[historySelection].url}); }
  else return false;
  e.preventDefault(); e.stopPropagation(); return true;
}
