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
  class Connection {
    static CONNECTING=0; static OPEN=1; static CLOSED=2;
    readyState=0;
  }
  class Socket extends Connection { static CLOSED=3; }
  const window = { WebSocket:Socket, EventSource:Connection, AudioContext: function() { Object.defineProperty(this,'state',{get:()=>context.state}); }, RTCPeerConnection:function() { Object.defineProperty(this,'connectionState',{get:()=>peer.connectionState}); } };
  const navigator = { mediaDevices: { getUserMedia:async()=>stream, getDisplayMedia:async()=>stream } };
  const document = { addEventListener:(name,cb)=>handlers[name]=cb, querySelectorAll:query=>elements[query] };
  vm.runInNewContext(code, { window,navigator,document,Set,Proxy,Reflect });
  return {window,navigator,handlers,elements,context,stream,peer,busy:()=>window.__axolotlSleepBusy()};
}
(async()=>{
 let p=page(); assert.equal(p.busy(),false);
 p.handlers.input({isTrusted:false}); assert.equal(p.busy(),false);
 p.handlers.input({isTrusted:true}); assert.equal(p.busy(),true);
 // A prevented/asynchronous submit is not proof the server saved the edit.
 p.handlers.submit?.(); assert.equal(p.busy(),true);
 p=page();
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
 for(const name of ['WebSocket','EventSource']) {
   p=page(); const Native=p.window[name];
   class CustomConnection extends p.window[name] {}
   const connection=new CustomConnection('https://example.test');
   assert.ok(connection instanceof Native); assert.ok(connection instanceof CustomConnection);
   assert.equal(p.window[name].OPEN,1);
   assert.equal(p.window[name].CLOSED,name==='WebSocket'?3:2);
   assert.equal(p.busy(),true,`${name} connecting must stay awake`);
   connection.readyState=1; assert.equal(p.busy(),true,`${name} open must stay awake`);
   connection.readyState=2;
   // WebSocket CLOSED is 3, unlike EventSource CLOSED (2).
   if(name==='WebSocket') { assert.equal(p.busy(),true); connection.readyState=3; }
   assert.equal(p.busy(),false,`${name} closed can sleep`);
 }
 console.log('Sleep activity checks passed: unsaved submits, live connections, trusted edits, media, calls, audio contexts, frames');
})().catch(error=>{console.error(error);process.exit(1)});
