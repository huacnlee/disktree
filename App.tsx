import { mount, signal, computed, effect, cleanup, batch, View, Text, Input, Scroll, type YNode } from 'nsh';
import { Animated, createNativeValue, LayoutAnimation } from 'nsh/animated';
import { reduceMotion } from 'nsh/accessibility';
import { Window, titleBar } from 'nsh/desktop';
import { keymap } from 'nsh/keymap';
import { endpoint, token, initialPath } from './runtime';
import { type DiskNode, type Snapshot, type DirectoryView, type Metric, mosaic, formatSize, kind, categories } from './disk';

import { palette as C, finish, Button, MapTile, EmptyMap, MetricControl, AppMark, mono } from './ui';
function App() {
  Window.setTitle('Disktree — Disk space explorer');
  Window.setTitleBar('hidden', 56);
  const snapshot = signal<Snapshot | null>(null);
  const current = signal<DiskNode | null>(null);
  const selection = signal<DiskNode | null>(null);
  const path = signal(initialPath);
  let pathField:YNode | undefined;
  let filterField:YNode | undefined;
  const query = signal('');
  const busy = signal(false);
  const message = signal('Choose a folder to discover what fills your disk.');
  const hidden = signal(true);
  const metric = signal<Metric>('size');
  const levels = signal(2);
  const marked = signal<DiskNode[]>([]);
  const review = signal(false);
  const pointerInMap=signal(false);
  const hovered=signal(new Map<string,DiskNode>());
  function hoverNode(node:DiskNode,inside:boolean){hovered.update(previous=>{const next=new Map(previous);if(inside)next.set(node.path,node);else next.delete(node.path);return next;});}
  function markAtPointer(){const nodes=Array.from(hovered().values()).sort((a,b)=>b.path.length-a.path.length);toggleMark(nodes[0] || (pointerInMap()?null:selection()));}
  const frame = signal({ width: 700, height: 520 }, { equals: (a,b)=>a.width===b.width && a.height===b.height });
  const zoom = signal(1);
  const cameraScale=createNativeValue(1);
  const cameraX=createNativeValue(0);
  const cameraY=createNativeValue(0);
  const boundedScale=cameraScale.interpolate({inputRange:[1,8],outputRange:[1,8]});
  const cameraTransform=computed(()=>{
    const maxX=boundedScale.sub(1).mul(frame().width/2);
    const maxY=boundedScale.sub(1).mul(frame().height/2);
    return [{translateX:cameraX.div(maxX.add(0.001)).interpolate({inputRange:[-1,1],outputRange:[-1,1]}).mul(maxX)},{translateY:cameraY.div(maxY.add(0.001)).interpolate({inputRange:[-1,1],outputRange:[-1,1]}).mul(maxY)},{scale:boundedScale}];
  });
  const detailLevels = computed(()=>Math.max(levels(),zoom()>=1.8?3:1));
  const listing = signal<DirectoryView | null>(null);
  const directoryPath = signal('');
  const page = signal(0);
  const browseBusy = signal(false);
  let viewGeneration = 0;
  const entrance = createNativeValue(0);
  const sweep = createNativeValue(0);
  const panelReveal = createNativeValue(1);
  function motionLayout() {
    if (!reduceMotion()) LayoutAnimation.configureNext({ duration: 380, create: { type: 'easeOut', property: 'opacity', duration: 300 }, update: { type: 'spring', springDamping: 0.9 }, delete: { type: 'easeOut', property: 'opacity', duration: 160 } });
  }
  effect(() => {
    if (reduceMotion()) entrance.set(1);
    else void entrance.timing(1, { duration: 680, easing: [0.16,1,0.3,1] });
  });
  effect(() => {
    review();
    selection()?.path;
    if (reduceMotion()) { panelReveal.set(1); return; }
    panelReveal.set(0);
    void panelReveal.timing(1, { duration: 220, easing: [0.16,1,0.3,1] });
  });
  effect(() => {
    if (!busy() || reduceMotion()) { sweep.set(0); return; }
    const loop = Animated.loop(Animated.timing(sweep, { toValue: 1, duration: 1700, easing: 'linear' }));
    void loop.start();
    cleanup(() => loop.stop());
  });
  let generation = 0;
  async function request(route: string, data: unknown) {
    const response = await fetch(endpoint + route, { method: 'POST', headers: { Authorization: 'Bearer ' + token, 'Content-Type': 'application/json' }, body: JSON.stringify(data) });
    const result = await response.json();
    if (!response.ok) throw new Error(result.error || 'Request failed');
    return result;
  }
  async function scan(target = path()) {
    if (busy()) return;
    const run = ++generation;
    busy.set(true); message.set('Scanning… large folders may take a moment.');
    try {
      const data: Snapshot = await request('/scan', { path: target });
      if (run !== generation) return;
      batch(()=>{listing.set(null);snapshot.set(data);current.set(data.root);selection.set(data.root);marked.set([]);review.set(false);path.set(data.root.path);directoryPath.set(data.root.path);page.set(0);query.set('');});
      message.set((data.errorCount || data.errors.length) ? `${data.errorCount || data.errors.length} unreadable or skipped paths. Scan totals exclude these items.` : 'Scan complete · symbolic links and other volumes are not followed.');
    } catch (error) { message.set(`${error instanceof Error ? error.message : error}. Start the app with scripts/run.sh and try again.`); }
    finally { busy.set(false); }
  }
  const startupScan=setTimeout(()=>void scan(),0);
  cleanup(()=>clearTimeout(startupScan));
  function enter(node: DiskNode) {
    if(node.aggregate) { message.set('Use the directory list and page controls to explore the remaining items.'); return; }
    selection.set(node);
    if(node.directory) batch(()=>{directoryPath.set(node.path);page.set(0);query.set('');});
  }
  function selectOrEnter(node:DiskNode){
    if(node.aggregate || (selection()?.path===node.path && node.directory))enter(node);
    else selection.set(node);
  }
  effect(() => {
    const data = snapshot();
    const folder = directoryPath();
    const parameters = { id: data?.id, path: folder, offset: page(), query: query(), hidden: hidden(), metric: metric(), levels: detailLevels() };
    if (!data || !folder) return;
    const run = ++viewGeneration;
    browseBusy.set(true);
    void request('/view', parameters).then((result: DirectoryView) => {
      if (run !== viewGeneration) return;
      motionLayout();
      batch(()=>{listing.set(result);current.set(result.node);path.set(result.node.path);browseBusy.set(false);});
    }).catch(error=>{
      if(run===viewGeneration){browseBusy.set(false);message.set(String(error));}
    });
  });
  function up() {
    if(busy() || browseBusy()) return;
    const folder=current()?.path;
    if(!folder || folder==='/') return;
    const ancestors=listing()?.ancestors || [];
    if(ancestors.length>1) enter(ancestors[ancestors.length-2]);
    else void scan(folder.slice(0,folder.lastIndexOf('/')) || '/');
  }
  function toggleMark(node: DiskNode | null) {
    if (!node || node.aggregate || node.path === snapshot()?.root.path || busy() || browseBusy()) return;
    motionLayout();
    if (marked().some(n => n.path === node.path)) { marked.set(marked().filter(n => n.path !== node.path)); return; }
    if (marked().some(n => node.path.startsWith(n.path + '/'))) { message.set('A parent folder is already marked.'); return; }
    marked.set([...marked().filter(n => !n.path.startsWith(node.path + '/')),node]);
  }
  const markedBytes = () => marked().reduce((sum,n) => sum+n.size,0);
  const children = computed(() => listing()?.children || []);
  const tiles = computed(() => mosaic(listing()?.map || [],frame().width,frame().height,metric(),detailLevels(),zoom()));
  const crumbs = computed(() => listing()?.ancestors || []);
  function changePage(offset: number) { page.set(Math.max(0,offset)); }
  function zoomMap(scale: number) {
    const next=Math.max(1,Math.min(8,scale));
    zoom.set(next);
    if(reduceMotion())cameraScale.set(next);
    else void cameraScale.timing(next,{duration:220,easing:[0.16,1,0.3,1]});
    if(next===1){cameraX.set(0);cameraY.set(0);}
  }
  function endPinch(scale: number) {
    const next=Math.max(1,Math.min(8,scale));
    cameraScale.set(next);zoom.set(next);
    if(next===1){cameraX.set(0);cameraY.set(0);}
  }
  effect(()=>{directoryPath();page();metric();hovered.set(new Map());zoom.set(1);cameraScale.set(1);cameraX.set(0);cameraY.set(0);});
  async function commit() {
    if (busy() || !snapshot() || !marked().length) return;
    busy.set(true);
    try {
      const result = await request('/trash', { id: snapshot()!.id, paths: marked().map(n=>n.path) });
      marked.set(marked().filter(n=>!result.completed.includes(n.path)));
      review.set(false);
      message.set(result.error || `${result.completed.length} items moved to Trash. Rescan to update disk usage. Empty Trash in Finder to reclaim space.`);
    } catch (error) { message.set(String(error)); }
    finally { busy.set(false); }
  }
  keymap.bind({ 'mod+plus':()=>zoomMap(zoom()*1.5),'mod+minus':()=>zoomMap(zoom()/1.5),'mod+0':()=>zoomMap(1), 'alt+up': up, 'enter': () => { if (selection()) enter(selection()!); }, 'space': markAtPointer, 'escape': () => review.set(false), 'mod+r': () => { void scan(); }, 'down': () => {
    const list = children(); const i = list.findIndex(n=>n.path===selection()?.path); if (list.length) selection.set(list[Math.min(i+1,list.length-1)]);
  }, 'up': () => { const list=children(); const i=list.findIndex(n=>n.path===selection()?.path); if(list.length) selection.set(list[Math.max(i-1,0)]); } }, { when: 'notEditing' });
  return <View style={{ flex:1,backgroundColor:C.bg,opacity:entrance }}>
    <View desktop={{windowDrag:true}} testID="titlebar" style={{height:56,flexDirection:'row',alignItems:'center',gap:10,paddingLeft:Math.max(16,titleBar().left+8),paddingRight:Math.max(16,titleBar().right+8),borderBottomWidth:1,borderColor:C.line,backgroundColor:'#111113',...finish}}>
      <AppMark/>
      <Text style={{fontFamily:mono,fontSize:17,fontWeight:'600',letterSpacing:-0.5,color:C.text}}>disktree</Text>
      <Input ref={node=>{pathField=node;}} accessibilityLabel="Folder path" value={path()} onChangeText={v=>path.set(v)} onSubmitEditing={()=>void scan()} autoCorrect={false} autoCapitalize="none" placeholder="Folder path…" style={{fontFamily:mono,flex:1,color:C.text,fontSize:12,padding:8,backgroundColor:'#09090b',borderWidth:1,borderColor:C.line,borderRadius:6}}/>
      <Button label={busy()?'Working…':'Scan folder'} action={()=>void scan()} active/>
      <Button label="↑ Parent" action={up}/>
      <MetricControl value={metric()} change={value=>batch(()=>{metric.set(value);page.set(0);})}/>
      <Button label={hidden()?'Hidden: on':'Hidden: off'} action={()=>batch(()=>{hidden.update(v=>!v);page.set(0);})}/>
      <Button label={`Depth ${levels()}`} action={()=>{motionLayout();levels.set(levels()===3?1:levels()+1);}}/>
    </View>
    <View style={{height:2,overflow:'hidden',backgroundColor:busy()?C.line:'transparent'}}>
      <If when={busy()}><View style={{height:2,width:reduceMotion()?'100%':'28%',backgroundColor:C.amber,transform:[{translateX:sweep.interpolate({inputRange:[0,1],outputRange:[-300,1500]})}]}}/></If>
    </View>
    <View style={{flex:1,flexDirection:'row',minHeight:150}}>
      <View style={{flex:1,padding:10,gap:7}}>
        <View style={{flexDirection:'row',alignItems:'center',gap:8,height:28}}>
          <Each of={crumbs()}>{(n,i)=><Text accessibilityRole="button" accessibilityState={{disabled:i()===crumbs().length-1}} accessibilityLabel={i()===crumbs().length-1?`Current folder: ${n().name}`:`Go to ${n().name}`} onPress={i()===crumbs().length-1?undefined:()=>enter(n())} numberOfLines={1} style={{fontFamily:mono,fontSize:12,color:i()===crumbs().length-1?C.muted:C.amber,cursor:i()===crumbs().length-1?'default':'pointer'}}>{n().name}{i()===crumbs().length-1?'':' /'}</Text>}</Each>
          <View style={{flex:1}}/>
          <Text style={{fontFamily:mono,fontSize:11,color:C.muted}}>{current()?`${formatSize(current()!.size)} · ${current()!.files.toLocaleString()} files`:''}{browseBusy()?' · Loading…':''}</Text>
          <Input ref={node=>{filterField=node;}} value={query()} onChangeText={v=>batch(()=>{query.set(v);page.set(0);})} placeholder="Filter names…" placeholderTextColor={C.muted} style={{fontFamily:mono,width:130,fontSize:11,color:C.text,padding:6,backgroundColor:'#18181c',borderRadius:4}}/>
        </View>
        <Scroll horizontal style={{height:20,flexGrow:0,flexShrink:0}}><View style={{flexDirection:'row',gap:13,alignItems:'center'}}><Each of={categories}>{category=><View style={{flexDirection:'row',alignItems:'center',gap:4}}><View style={{width:8,height:8,borderRadius:1,backgroundColor:category().color}}/><Text style={{fontFamily:mono,fontSize:10,color:C.muted}}>{category().label}</Text></View>}</Each></View></Scroll>
        <View testID="treemap" class="hover:opacity-100" {...{onHoverIn:()=>{pointerInMap.set(true);pathField?.blur();filterField?.blur();},onHoverOut:()=>{pointerInMap.set(false);hovered.set(new Map());}}} pinch={{scale:cameraScale}} pan={{x:cameraX,y:cameraY}} onPinchEnd={()=>{void cameraScale.getValue().then(endPinch);}} onLayout={e=>frame.set({width:e.width,height:e.height})} style={{flex:1,overflow:'hidden',backgroundColor:'#050506',borderRadius:3}}>
          <View style={{width:frame().width,height:frame().height,transform:cameraTransform()}}><Each of={tiles()} key={tile=>tile.node.path}>{(tile,i)=><MapTile tile={tile()} order={i()} selected={selection()?.path===tile().node.path} marked={marked().some(n=>n.path===tile().node.path || tile().node.path.startsWith(n.path+'/'))} dimmed={!!selection() && selection()?.path!==current()?.path && selection()?.path!==tile().node.path} metric={metric()} levels={detailLevels()} zoom={zoom()} hover={()=>hoverNode(tile().node,true)} leave={()=>hoverNode(tile().node,false)} choose={()=>selectOrEnter(tile().node)} open={()=>enter(tile().node)}/>}</Each>
          </View>
          <If when={!tiles().length}><View pointerEvents="none" style={{position:'absolute',left:0,right:0,top:0,bottom:0}}><EmptyMap busy={busy()}/></View></If>
        </View>
      </View>
      <View style={{width:284,backgroundColor:C.panel,borderLeftWidth:1,borderColor:C.line}}>
        <Scroll style={{flex:1}}>
          <View style={{padding:16,gap:12,opacity:panelReveal,transform:[{translateX:panelReveal.interpolate({inputRange:[0,1],outputRange:[8,0]})}]}}>
            <Text style={{fontFamily:mono,fontSize:10,letterSpacing:1,color:C.muted}}>{review()?'REVIEW MARKED ITEMS':'SELECTION'}</Text>
            <If when={!review()}>
              <View style={{flexDirection:'row',alignItems:'center',gap:8}}><View style={{width:3,height:19,backgroundColor:selection()?kind(selection()!).color:C.amber}}/><Text style={{fontFamily:mono,fontSize:20,fontWeight:'600',color:C.text,flex:1}} numberOfLines={1}>{selection()?.name || 'Nothing selected'}</Text></View>
              <Text style={{fontFamily:mono,fontSize:11,color:C.muted}}>{selection()?.path || 'Select a tile to inspect it.'}</Text>
              <Text style={{fontFamily:mono,fontSize:38,fontWeight:'500',letterSpacing:-1,color:'#e8e8ed',fontVariant:['tabular-nums']}}>{formatSize(selection()?.size || 0)}</Text>
              <View style={{height:4,backgroundColor:'#26262b',borderRadius:2}}><View style={{height:4,width:`${100*(selection()?.size || 0)/(snapshot()?.root.size || 1)}%`,backgroundColor:selection()?kind(selection()!).color:C.amber,borderRadius:2}}/></View>
              <View style={{flexDirection:'row',gap:16}}><View style={{flex:1,gap:5}}><Text style={{fontFamily:mono,fontSize:10,color:C.muted}}>OF SCAN</Text><Text style={{fontFamily:mono,fontSize:15,color:C.text}}>{((selection()?.size || 0)/(snapshot()?.root.size || 1)*100).toFixed(1)}%</Text></View><View style={{flex:1,gap:5}}><Text style={{fontFamily:mono,fontSize:10,color:C.muted}}>FILES</Text><Text style={{fontFamily:mono,fontSize:15,color:C.text}}>{(selection()?.files || 0).toLocaleString()}</Text></View></View>
              <View style={{flexDirection:'row',gap:16}}><View style={{flex:1,gap:5}}><Text style={{fontFamily:mono,fontSize:10,color:C.muted}}>MODIFIED</Text><Text style={{fontFamily:mono,fontSize:12,color:C.text}}>{selection()?new Date(selection()!.modified*1000).toLocaleDateString():'—'}</Text></View><View style={{flex:1,gap:5}}><Text style={{fontFamily:mono,fontSize:10,color:C.muted}}>KIND</Text><Text style={{fontFamily:mono,fontSize:12,color:C.text}}>{selection()?kind(selection()!).label:'—'}</Text></View></View>
              <View style={{flexDirection:'row',gap:6}}><Button label="Open folder" action={()=>{if(selection())enter(selection()!);}}/><Button label="Mark / unmark" action={()=>toggleMark(selection())}/></View>
            </If>
            <If when={review()}>
              <Text style={{fontFamily:mono,fontSize:15,color:C.text}}>Move these items to Trash?</Text>
              <Text style={{fontFamily:mono,fontSize:11,color:C.muted}}>Recoverable from Finder. Space is reclaimed when Trash is emptied.</Text>
              <Each of={marked()}>{node=><View style={{gap:7,paddingVertical:6}}><Text style={{fontFamily:mono,fontSize:11,color:C.text}}>{node().path}</Text><Button label={`Remove mark · ${formatSize(node().size)}`} action={()=>toggleMark(node())}/></View>}</Each>
              <Button label={busy()?'Moving…':'Confirm move to Trash'} action={()=>void commit()} active/>
              <Button label="Back to explorer" action={()=>review.set(false)}/>
            </If>
            <View style={{height:1,backgroundColor:C.line}}/>
            <Text style={{fontFamily:mono,fontSize:12,color:C.text}}>{marked().length} marked · {formatSize(markedBytes())}</Text>
            <If when={!review()}><Button label="Review marked items →" action={()=>{if(marked().length)review.set(true);}} active={marked().length>0}/></If>
            <If when={!review()}>
              <View style={{height:1,backgroundColor:C.line}}/>
              <View style={{flexDirection:'row',alignItems:'center',gap:5}}><Text style={{fontFamily:mono,flex:1,fontSize:10,letterSpacing:0.8,color:C.muted}}>CONTENTS · {listing()?.count || 0} ITEMS</Text><Text style={{fontFamily:mono,fontSize:10,color:C.muted}}>{listing()?.count?`${listing()!.offset+1}–${Math.min(listing()!.offset+listing()!.pageSize,listing()!.count)}`:''}</Text></View>
              <View style={{flexDirection:'row',justifyContent:'space-between'}}><Button label="‹ Previous" action={()=>changePage(page()-(listing()?.pageSize || 80))}/><Button label="Next ›" action={()=>{const v=listing();if(v && v.offset+v.pageSize<v.count)changePage(v.offset+v.pageSize);}}/></View>
              <Each of={children()}>{node=><View onPress={()=>selectOrEnter(node())} class="hover:brightness-125" style={{paddingVertical:7,gap:5,cursor:'pointer',borderBottomWidth:1,borderColor:'#242428'}}><View style={{flexDirection:'row',gap:6,alignItems:'center'}}><View style={{width:3,height:13,backgroundColor:kind(node()).color}}/><Text numberOfLines={1} style={{fontFamily:mono,flex:1,fontSize:11,color:selection()?.path===node().path?C.amber:C.text}}>{marked().some(n=>n.path===node().path)?'✓ ':''}{node().directory?'▸ ':''}{node().name}</Text><Text style={{fontFamily:mono,fontSize:10,color:C.muted}}>{metric()==='files'?String(node().files):formatSize(node()[metric()])}</Text></View><View style={{height:2,backgroundColor:'#101012'}}><View style={{height:2,width:`${100*node()[metric()]/(current()?.[metric()] || 1)}%`,backgroundColor:kind(node()).color}}/></View></View>}</Each>
            </If>
          </View>
        </Scroll>
        <View style={{padding:16,gap:9,borderTopWidth:1,borderColor:C.line,backgroundColor:'#141416',...finish}}>
          <Text style={{fontFamily:mono,fontSize:10,letterSpacing:1,color:C.muted}}>DISK SPACE</Text>
          <Text style={{fontFamily:mono,fontSize:25,fontWeight:'500',color:C.text,fontVariant:['tabular-nums']}}>{snapshot()?formatSize(snapshot()!.free):'—'}<Text style={{fontFamily:mono,fontSize:12,color:C.muted}}> free</Text></Text>
          <View style={{height:5,backgroundColor:'#28282d',borderRadius:2}}><View style={{height:5,width:`${snapshot()?100*(1-snapshot()!.free/snapshot()!.total):0}%`,backgroundColor:'#56b675',borderRadius:2}}/></View>
          <View style={{flexDirection:'row',justifyContent:'space-between'}}><Text style={{fontFamily:mono,fontSize:10,color:C.muted}}>{snapshot()?`${formatSize(snapshot()!.total-snapshot()!.free)} used`:''}</Text><Text style={{fontFamily:mono,fontSize:10,color:C.muted}}>{snapshot()?`${formatSize(snapshot()!.total)} total`:''}</Text></View>
        </View>
      </View>
    </View>
    <View style={{flexDirection:'row',gap:16,alignItems:'center',paddingHorizontal:12,paddingVertical:6,borderTopWidth:1,borderColor:C.line,backgroundColor:'#101012'}}><Text style={{fontFamily:mono,fontSize:10,color:C.muted}}>Pinch / ⌘± Zoom   ⌘0 Fit   Click selected Enter   Space Mark   ⌥↑ Parent   ⌘R Rescan   Esc Close review</Text><Text numberOfLines={1} style={{fontFamily:mono,flex:1,fontSize:10,color:'#85858f',textAlign:'right'}}>{message()}</Text></View>
  </View>;
}
mount(App);
