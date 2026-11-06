// Policy checks; native entry/playback is covered by --smoke-native-pip.
const assert = require('node:assert/strict');
const vm = require('node:vm');
const fs = require('node:fs');
const source = fs.readFileSync('crates/desktop/assets/native-pip.js', 'utf8');
function fixture(videos) {
  const messages = [], timers = [];
  class Video {
    constructor(area, options = {}) { Object.assign(this, {paused:false,ended:false,readyState:4,webkitPresentationMode:'inline',area,events:{}},options); }
    getBoundingClientRect() { return {width:this.area,height:1}; }
    webkitSupportsPresentationMode() { return true; }
    addEventListener(name, fn) { (this.events[name] ??= []).push(fn); }
    webkitSetPresentationMode(mode) { this.webkitPresentationMode=mode; this.events.webkitpresentationmodechanged?.forEach(fn=>fn()); }
  }
  const elements = videos.map(([area, opts])=>new Video(area, opts));
  const window={ipc:{postMessage:m=>messages.push(m.split(':').pop())}};
  vm.runInNewContext(source, {window,document:{querySelectorAll:()=>elements,addEventListener(){}},HTMLVideoElement:Video,setTimeout:fn=>timers.push(fn)});
  return {request:window.__axolotlNativePip,elements,messages,timers};
}
let f=fixture([[100],[400],[900,{paused:true}]]); f.request();
assert.equal(f.elements[1].webkitPresentationMode,'picture-in-picture');
assert.equal(f.elements[0].webkitPresentationMode,'inline');
assert.deepEqual(f.messages,['entered']);
f=fixture([[400,{disablePictureInPicture:true}],[100,{ended:true}]]);f.request();assert.deepEqual(f.messages,['unavailable']);
f=fixture([[100,{webkitPresentationMode:'picture-in-picture'}],[900]]);f.request();assert.equal(f.elements[1].webkitPresentationMode,'inline');
f=fixture([[100]]);f.elements[0].webkitSetPresentationMode=()=>{};f.request();f.timers.forEach(fn=>fn());assert.deepEqual(f.messages,['unavailable']);
f=fixture([[100]]);f.elements[0].webkitSetPresentationMode=()=>{throw Error('denied')};f.request();assert.deepEqual(f.messages,['unavailable']);
console.log('PASS: largest playing candidate, exclusions, existing PiP preserved, silent and thrown rejection');
f=fixture([[400],[100]]);f.request();
f.elements[1].webkitSetPresentationMode('fullscreen');
assert.ok(!f.messages.includes('left'));
f.elements[0].webkitSetPresentationMode('inline');
f.elements[0].events.leavepictureinpicture.forEach(fn=>fn());
assert.equal(f.messages.filter(m=>m==='left').length,1);
console.log('PASS: unrelated video presentation and duplicate exit events do not release ownership');
