// Execute the actual injected script with representative browser APIs.
const assert = require('node:assert/strict');
const vm = require('node:vm');
const fs = require('node:fs');
const code = fs.readFileSync('crates/desktop/assets/sleep-activity.js','utf8');
function page() {
  const handlers = {}, elements = { 'audio,video': [], 'iframe,frame': [] };
  const context = { state:'running' };
  const stream = { getTracks:()=>[{readyState:'live'}] };
  const peer = {connectionState:'connected'};
  const window = { AudioContext: function() { Object.defineProperty(this,'state',{get:()=>context.state}); }, RTCPeerConnection:function() { Object.defineProperty(this,'connectionState',{get:()=>peer.connectionState}); } };
  const navigator = { mediaDevices: { getUserMedia:async()=>stream, getDisplayMedia:async()=>stream } };
  const document = { addEventListener:(name,cb)=>handlers[name]=cb, querySelectorAll:query=>elements[query] };
  vm.runInNewContext(code, { window,navigator,document,Set,Proxy,Reflect });
  return {window,navigator,handlers,elements,context,stream,peer,busy:()=>window.__axolotlSleepBusy()};
}
(async()=>{
 let p=page(); assert.equal(p.busy(),false);
 p.handlers.input({isTrusted:false}); assert.equal(p.busy(),false);
 p.handlers.input({isTrusted:true}); assert.equal(p.busy(),true);
 p.handlers.submit(); assert.equal(p.busy(),false);
 p.elements['audio,video']=[{paused:false,ended:false}]; assert.equal(p.busy(),true);
 p.elements['audio,video'][0].paused=true; assert.equal(p.busy(),false);
 p=page(); await p.navigator.mediaDevices.getUserMedia(); assert.equal(p.busy(),true);
 p.stream.getTracks=()=>[{readyState:'ended'}]; assert.equal(p.busy(),false);
 p=page(); class CustomAudio extends p.window.AudioContext {}
 const audio=new CustomAudio(); assert.ok(audio instanceof CustomAudio); assert.equal(p.busy(),true);
 p.context.state='suspended'; assert.equal(p.busy(),false);
 p=page(); class CustomPeer extends p.window.RTCPeerConnection {}
 assert.ok(new CustomPeer() instanceof CustomPeer); assert.equal(p.busy(),true);
 p.peer.connectionState='closed'; assert.equal(p.busy(),false);
 p=page(); p.elements['iframe,frame']=[{contentWindow:{__axolotlSleepBusy:()=>true}}]; assert.equal(p.busy(),true);
 p.elements['iframe,frame']=[{get contentWindow(){throw Error('cross origin')}}]; assert.equal(p.busy(),true);
 console.log('Sleep activity checks passed: trusted edits, media, calls, audio contexts, frames');
})().catch(error=>{console.error(error);process.exit(1)});
