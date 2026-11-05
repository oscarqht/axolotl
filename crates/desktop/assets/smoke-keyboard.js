// Only fixture setup uses JavaScript. Editing is performed by AppKit NSEvents.
document.body.innerHTML = '<input id="input"><textarea id="textarea"></textarea><div id="editable" contenteditable="true"></div>';
for (const element of document.querySelectorAll('input,textarea,div')) {
  element.style.cssText = 'display:block;width:500px;height:80px;font:16px monospace;white-space:pre';
}
window.keyEvents = [];
window.cancelArrow = false;
for (const type of ['keydown', 'keyup', 'beforeinput']) {
  document.addEventListener(type, e => {
    keyEvents.push([e.type, e.key, e.isTrusted, e.data, e.repeat]);
    if (cancelArrow && e.type === 'keydown' && e.key.startsWith('Arrow')) e.preventDefault();
  });
}
window.prepare = (id, text = 'alpha beta gamma', offset = 10) => {
  window.editor = document.getElementById(id);
  if (editor.isContentEditable) {
    editor.textContent = text;
    editor.focus();
    const range = document.createRange();
    range.setStart(editor.firstChild, offset);
    range.collapse(true);
    getSelection().removeAllRanges();
    getSelection().addRange(range);
  } else {
    editor.value = text;
    editor.focus();
    editor.setSelectionRange(offset, offset);
  }
  keyEvents = [];
  return true;
};
window.state = () => ({
  value: editor.isContentEditable ? editor.textContent : editor.value,
  start: editor.isContentEditable ? Math.min(getSelection().anchorOffset, getSelection().focusOffset) : editor.selectionStart,
  end: editor.isContentEditable ? Math.max(getSelection().anchorOffset, getSelection().focusOffset) : editor.selectionEnd,
  events: keyEvents,
});
true;
