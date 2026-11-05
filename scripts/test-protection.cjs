const {test}=require('node:test');
const assert=require('node:assert/strict');
const vm=require('node:vm');
const fs=require('node:fs');
const source=fs.readFileSync('crates/desktop/assets/protection.js','utf8');
function load({enabled=true,exceptions=[],host='www.youtube.com',ad=false}={}) {
  const style={dataset:{},isConnected:false,textContent:''};
  const video={currentTime:0,duration:90};let skips=0;const timers=[];
  const player={classList:{contains:()=>ad},querySelector:s=>s==='video'?video:{click:()=>skips++}};
  const context={policy:{enabled,exceptions},filters:['##.advert','example.org#@#.advert','example.org##.specific','~example.org##.other','example.org##+js(unsafe)'],location:{hostname:host,protocol:'https:',href:'https://'+host+'/'},CSS:{supports:s=>!s.includes('+js(')},MutationObserver:class {observe(){}},document:{referrer:'',createElement:()=>style,documentElement:{append:s=>s.isConnected=true},querySelector:()=>player},setInterval:fn=>timers.push(fn),Response,Headers,URL,fetch:async()=>new Response('{"adPlacements":[1],"streamingData":{"formats":[1]},"playabilityStatus":{"status":"OK"}}'),JSON:Object.create(JSON)};
  context.window=context;context.top={location:context.location};vm.createContext(context);
  vm.runInContext('(()=>{'+source+'})()',context);
  return {context,style,video,timers,skips:()=>skips};
}
test('YouTube player response pruning preserves content playback fields',async()=>{
 const {context}=load();
 const value=vm.runInContext(`JSON.parse('{"adSlots":[1],"playerAds":[2],"streamingData":{"formats":[1]},"playabilityStatus":{"status":"OK"}}')`,context);
 assert.equal(value.adSlots,undefined);assert.equal(value.playerAds,undefined);assert.equal(value.streamingData.formats[0],1);assert.equal(value.playabilityStatus.status,'OK');
 vm.runInContext('ytInitialPlayerResponse={adPlacements:[1],videoDetails:{videoId:"content"}}',context);
 assert.equal(context.ytInitialPlayerResponse.adPlacements,undefined);assert.equal(context.ytInitialPlayerResponse.videoDetails.videoId,'content');
 const response=await context.fetch('/youtubei/v1/player');const data=await response.json();assert.equal(data.adPlacements,undefined);assert.ok(data.streamingData);
 const ordinary=await context.fetch('/ordinary.json');assert.deepEqual((await ordinary.json()).adPlacements,[1]);
});
test('cosmetics respect site scopes and selector exceptions',()=>{
 const {style}=load({host:'example.org'});assert.ok(style.textContent.includes('.specific{'));assert.ok(!style.textContent.includes('.advert{'));assert.ok(!style.textContent.includes('.other{'));assert.ok(!style.textContent.includes('+js('));
});
test('disabled policy and site exceptions leave page APIs and DOM untouched',()=>{
 for(const options of [{enabled:false},{exceptions:['www.youtube.com']}]) {
  const {context,style,timers}=load(options);assert.equal(style.isConnected,false);assert.equal(timers.length,0);assert.equal(context.JSON.parse,JSON.parse);
 }
});
test('skip fallback seeks only explicit finite ad video; never normal or live playback',()=>{
 const normal=load();normal.timers[0]();assert.equal(normal.video.currentTime,0);assert.equal(normal.skips(),0);
 const ad=load({ad:true});ad.timers[0]();assert.equal(ad.video.currentTime,90);assert.equal(ad.skips(),1);
 ad.video.duration=Infinity;ad.video.currentTime=0;ad.timers[0]();assert.equal(ad.video.currentTime,0);
});
