import { mount, signal, computed, effect, cleanup, batch, View, Text, Input, Scroll, type YNode } from 'nsh';
import { Animated, createAnimatedValue, LayoutAnimation } from 'nsh/animated';
import { reduceMotion } from 'nsh/accessibility';
import { Menu, Window, titleBar } from 'nsh/desktop';
import { keymap } from 'nsh/keymap';
import { request } from './runtime';
import { collapseBreadcrumbs } from './breadcrumbs';
import { type DiskNode, type Snapshot, type DirectoryView, type Metric, mosaic, formatSize, kind, categories } from './disk';

import { palette as C, finish, Button, MapTile, EmptyMap, MetricControl, AppMark, mono } from './ui';
function App() {
  Window.setTitle('Disktree — Disk space explorer');
  Window.setTitleBar('hidden', 56);
  const snapshot = signal<Snapshot | null>(null);
  const current = signal<DiskNode | null>(null);
  const selection = signal<DiskNode | null>(null);
  const path = signal('');
  let pathField:YNode | undefined;
  let filterField:YNode | undefined;
  let optionsButton:YNode | undefined;
  let breadcrumbOverflow:YNode | undefined;
  const crumbViewport=signal(0);
  const draftPath=signal('');
  const query = signal('');
  const busy = signal(false);
  const scanProgress = signal<{path:string;directories:number;files:number;bytes:number;errors:number} | null>(null);
  const scanSeconds = signal(0);
  let stopScanProgress: (()=>void) | undefined;
  cleanup(()=>stopScanProgress?.());
  async function scanWithProgress(target:string):Promise<Snapshot> {
    const started = Date.now();
    const progressToken = `${started}-${++scanSequence}`;
    let stopped = false;
    let timer:ReturnType<typeof setTimeout> | undefined;
    const stop = ()=>{stopped=true;if(timer!==undefined)clearTimeout(timer);scanProgress.set(null);};
    stopScanProgress?.();stopScanProgress=stop;
    batch(()=>{scanSeconds.set(0);scanProgress.set({path:target || 'Opening saved folder…',directories:0,files:0,bytes:0,errors:0});});
    async function poll() {
      try {
        const progress = await request('/scan-progress',{progressToken});
        if(stopped)return;
        batch(()=>{if(progress)scanProgress.set(progress);scanSeconds.set(Math.floor((Date.now()-started)/1000));});
      } catch { /* A progress update must not interrupt the scan. */ }
      if(!stopped)timer=setTimeout(()=>void poll(),200);
    }
    timer=setTimeout(()=>void poll(),100);
    try { return await request('/scan',{path:target,progressToken}); }
    finally { stop();if(stopScanProgress===stop)stopScanProgress=undefined; }
  }
  let scanSequence = 0;
  const message = signal('Choose a folder to discover what fills your disk.');
  const hidden = signal(true);
  const metric = signal<Metric>('size');
  const levels = signal(2);
  async function showOptions() {
    const anchor = await optionsButton?.measure();
    await Menu.popup([
      { label:'Show hidden files', checked:hidden(), onSelect:()=>batch(()=>{hidden.update(value=>!value);page.set(0);}) },
      { type:'separator' },
      { label:'Treemap depth', submenu:[1,2,3].map(depth=>({
        label:`${depth} ${depth===1?'level':'levels'}`,
        checked:levels()===depth,
        onSelect:()=>levels.set(depth),
      })) },
    ], anchor?{x:anchor.pageX,y:anchor.pageY+anchor.height+4}:undefined);
  }
  const marked = signal<DiskNode[]>([]);
  const review = signal(false);
  const removal = signal<'trash'|'delete'|'empty'>('trash');
  const editingPath = signal(false);
  const reviewed=signal<DiskNode[]>([]);
  function beginReview(mode:'trash'|'delete'|'empty',nodes=marked()) {
    if(busy() || browseBusy() || (mode!=='empty' && !nodes.length))return;
    batch(()=>{removal.set(mode);reviewed.set(nodes.slice());review.set(true);});
  }
  const pointerInMap=signal(false);
  const hovered=signal(new Map<string,DiskNode>());
  function hoverNode(node:DiskNode,inside:boolean){hovered.update(previous=>{if(previous.has(node.path)===inside)return previous;const next=new Map(previous);if(inside)next.set(node.path,node);else next.delete(node.path);return next;});}
  function markAtPointer(){const nodes=Array.from(hovered().values()).sort((a,b)=>b.path.length-a.path.length);toggleMark(nodes[0] || (pointerInMap()?null:selection()));}
  const frame = signal({ width: 700, height: 520 }, { equals: (a,b)=>a.width===b.width && a.height===b.height });
  const zoom = signal(1);
  const cameraScale=createAnimatedValue(1);
  const cameraX=createAnimatedValue(0);
  const cameraY=createAnimatedValue(0);
  const boundedScale=cameraScale.interpolate({inputRange:[1,8],outputRange:[1,8]});
  const cameraTransform=computed(()=>{
    const maxX=boundedScale.sub(1).mul(frame().width/2);
    const maxY=boundedScale.sub(1).mul(frame().height/2);
    return [{translateX:cameraX.div(maxX.add(0.001)).interpolate({inputRange:[-1,1],outputRange:[-1,1]}).mul(maxX)},{translateY:cameraY.div(maxY.add(0.001)).interpolate({inputRange:[-1,1],outputRange:[-1,1]}).mul(maxY)},{scale:boundedScale}];
  });
  const detailLevels = computed(()=>Math.max(levels(),zoom()>=1.8?3:1));
  const listing = signal<DirectoryView | null>(null);
  // Keep the rendered metric with its response. The control can respond
  // immediately without rearranging the old page before the new one arrives.
  const listingMetric = signal<Metric>('size');
  const directoryPath = signal('');
  const page = signal(0);
  const browseBusy = signal(false);
  let viewGeneration = 0;
  let loadedViewKey = '';
  function viewParameters(id:string|undefined,folder:string,offset=page()) {
    return {id,path:folder,offset,query:query(),hidden:hidden(),metric:metric(),levels:detailLevels()};
  }
  const entrance = createAnimatedValue(0);
  const sweep = createAnimatedValue(0);
  const panelReveal = createAnimatedValue(1);
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
  async function scan(target = path(), restoreFolder?:string) {
    if (busy()) return;
    const run = ++generation;
    ++viewGeneration;
    browseBusy.set(false);
    busy.set(true); message.set('Scanning… large folders may take a moment.');
    try {
      const data: Snapshot = await scanWithProgress(target);
      if (run !== generation) return;
      batch(()=>{listing.set(null);snapshot.set(data);current.set(data.root);selection.set(data.root);marked.set([]);review.set(false);path.set(data.root.path);directoryPath.set(restoreFolder || data.root.path);page.set(0);query.set('');});
      message.set((data.errorCount || data.errors.length) ? `${data.errorCount || data.errors.length} unreadable or skipped paths. Scan totals exclude these items.` : 'Scan complete · symbolic links and other volumes are not followed.');
    } catch (error) { message.set(`${error instanceof Error ? error.message : error}. Restart the app and try again.`); }
    finally { busy.set(false); }
  }
  function enter(node: DiskNode) {
    if(busy() || browseBusy())return;
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
    const parameters = viewParameters(data?.id,folder);
    const key=JSON.stringify(parameters);
    if (busy() || !data || !folder || key===loadedViewKey) return;
    const run = ++viewGeneration;
    browseBusy.set(true);
    void request('/view', parameters).then((result: DirectoryView) => {
      if (run !== viewGeneration) return;
      loadedViewKey=JSON.stringify({...parameters,path:result.node.path,offset:result.offset});
      batch(()=>{listingMetric.set(parameters.metric);listing.set(result);current.set(result.node);if(selection()?.path===data.root.path)selection.set(result.node);path.set(result.node.path);browseBusy.set(false);});
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
    if (marked().some(n => n.path === node.path)) { marked.set(marked().filter(n => n.path !== node.path)); return; }
    if (marked().some(n => node.path.startsWith(n.path + '/'))) { message.set('A parent folder is already marked.'); return; }
    marked.set([...marked().filter(n => !n.path.startsWith(node.path + '/')),node]);
  }
  const markedBytes = () => marked().reduce((sum,n) => sum+n.size,0);
  const children = computed(() => listing()?.children || []);
  const tiles = computed(() => mosaic(listing()?.map || [],frame().width,frame().height,listingMetric(),detailLevels(),zoom()));
  let previousTiles: ReturnType<typeof mosaic> = [];
  effect(() => {
    const next = tiles();
    const changed = next.length !== previousTiles.length || next.some((tile, i) => {
      const old = previousTiles[i];
      return !old || tile.node.path !== old.node.path || tile.x !== old.x || tile.y !== old.y || tile.width !== old.width || tile.height !== old.height;
    });
    previousTiles = next;
    if (changed && !reduceMotion()) {
      // Animate the commit that changes the keyed tiles. Native layout handles
      // positions/sizes; no JS frame loop or idle animation is needed.
      LayoutAnimation.configureNext({
        duration: 320,
        create: { type:'easeOut', property:'opacity' },
        update: { type:'easeInEaseOut' },
        delete: { type:'easeOut', property:'opacity', duration:160 },
      });
    }
  });
  const crumbs = computed(() => {
    const parts=path().split('/').filter(Boolean);
    return [{name:'/',path:'/'},...parts.map((name:string,i:number)=>({name,path:'/'+parts.slice(0,i+1).join('/')}))];
  });
  const collapsedCrumbs=computed(()=>collapseBreadcrumbs(crumbs(),crumbViewport()));
  async function showBreadcrumbs() {
    const anchor=await breadcrumbOverflow?.measure();
    await Menu.popup(collapsedCrumbs().hidden.map(crumb=>({
      label:crumb.path,
      enabled:!busy() && !browseBusy(),
      onSelect:()=>navigate(crumb.path),
    })),anchor?{x:anchor.pageX,y:anchor.pageY+anchor.height+4}:undefined);
  }
  function navigate(target:string) {
    if(busy() || browseBusy())return;
    const node=listing()?.ancestors.find(n=>n.path===target);
    if(node)enter(node);else void scan(target);
  }
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
  effect(()=>{directoryPath();page();listingMetric();hovered.set(new Map());zoom.set(1);cameraScale.set(1);cameraX.set(0);cameraY.set(0);});
  async function commit() {
    if (busy() || browseBusy() || (removal()!=='empty' && (!snapshot() || !reviewed().length))) return;
    const folder=current()?.path || path();
    const parameters=viewParameters(undefined,folder);
    ++viewGeneration;
    busy.set(true);
    let outcome='';
    try {
      const mode=removal();
      const result = await request(mode==='empty'?'/empty-trash':mode==='delete'?'/delete':'/trash', { id: snapshot()?.id, paths: reviewed().map(n=>n.path) });
      outcome=result.error || (mode==='empty'?'Trash emptied.':`${result.completed.length} items ${mode==='delete'?'permanently deleted':'moved to Trash'}.`);
      const completed:string[] = Array.isArray(result.completed)?result.completed:[];
      const removed=(node:DiskNode)=>completed.some(p=>node.path===p || node.path.startsWith(p+'/'));
      // The native response already contains the updated cached index. Keep the
      // current map visible and fetch only its bounded directory view.
      if(result.snapshot) {
        const data:Snapshot={...snapshot()!,...result.snapshot};
        const updated:DirectoryView=await request('/view',{...parameters,id:data.id,fallback:true});
        loadedViewKey=JSON.stringify({...parameters,id:data.id,path:updated.node.path,offset:updated.offset});
        batch(()=>{snapshot.set(data);listingMetric.set(parameters.metric);listing.set(updated);current.set(updated.node);selection.set(updated.node);path.set(updated.node.path);directoryPath.set(updated.node.path);page.set(updated.offset);});
      }
      batch(()=>{review.set(!!result.error);reviewed.set(reviewed().filter(n=>!removed(n)));marked.set(marked().filter(n=>!removed(n)));hovered.set(new Map());});
      if(result.warning)outcome+=` ${result.warning}`;
      message.set(outcome);
    } catch (error) {
      message.set(outcome?`${outcome} Refresh failed: ${String(error)}. Scan again to update disk usage.`:String(error));
    } finally { busy.set(false); }
  }
  keymap.bind({ 'mod+plus':()=>zoomMap(zoom()*1.5),'mod+minus':()=>zoomMap(zoom()/1.5),'mod+0':()=>zoomMap(1), 'alt+up': up, 'enter': () => { if (selection()) enter(selection()!); }, 'space': markAtPointer, 'mod+r': () => { void scan(); }, 'down': () => {
    const list = children(); const i = list.findIndex(n=>n.path===selection()?.path); if (list.length) selection.set(list[Math.min(i+1,list.length-1)]);
  }, 'up': () => { const list=children(); const i=list.findIndex(n=>n.path===selection()?.path); if(list.length) selection.set(list[Math.max(i-1,0)]); } }, { when: 'notEditing' });
  keymap.bind('escape',()=>{if(!busy()){review.set(false);editingPath.set(false);filterField?.blur();}}, {when:'always'});
  // Start the request during initialization so the first frame is already scanning.
  void scan();
  return <View style={{ flex:1,backgroundColor:C.bg,opacity:entrance }}>
    <View desktop={{windowDrag:true}} testID="titlebar" style={{height:56,flexDirection:'row',alignItems:'center',gap:10,paddingLeft:Math.max(16,titleBar().left+8),paddingRight:Math.max(16,titleBar().right+8),borderBottomWidth:1,borderColor:C.line,backgroundColor:'#111113',...finish}}>
      <AppMark/>
      <Text style={{fontFamily:mono,fontSize:17,fontWeight:'600',letterSpacing:-0.5,color:C.text}}>disktree</Text>
      <View style={{flex:1}}/>
      <Button label="Edit folder path" icon="edit" disabled={busy()} action={()=>{draftPath.set(path());editingPath.update(v=>!v);}}/>
      <Button label="Scan folder" icon="scan" disabled={busy() || browseBusy()} action={()=>{const target=editingPath()?draftPath():path();editingPath.set(false);void scan(target);}} active/>
      <Button label="Parent folder" icon="parent" disabled={busy() || browseBusy() || current()?.path==='/'} action={up}/>
      <MetricControl value={metric()} change={value=>batch(()=>{metric.set(value);page.set(0);})}/>
      <View ref={node=>{optionsButton=node;}}><Button label="View options" icon="options" action={()=>void showOptions()}/></View>
    </View>
    <View style={{height:2,overflow:'hidden',backgroundColor:busy()?C.line:'transparent'}}>
      <If when={busy()}><View style={{height:2,width:reduceMotion()?'100%':'28%',backgroundColor:C.amber,transform:[{translateX:sweep.interpolate({inputRange:[0,1],outputRange:[-300,1500]})}]}}/></If>
    </View>
    <View style={{flex:1,flexDirection:'row',minHeight:150}}>
      <View style={{flex:1,padding:10,gap:7}}>
        <View style={{flexDirection:'row',alignItems:'center',gap:8,height:28}}>
          <View desktop={{windowDrag:false}} style={{flex:1,minWidth:80,overflow:'hidden'}} onLayout={e=>crumbViewport.set(e.width)}>
            <If when={!editingPath()}>
              <View testID="breadcrumbs" style={{flexDirection:'row',alignItems:'center',gap:3,overflow:'hidden'}}>
                <View desktop={{windowDrag:false}} role="button" accessibilityLabel="Go to /" accessibilityState={{disabled:busy() || browseBusy() || path()==='/'}} onPress={()=>navigate('/')} class="hover:brightness-125" style={{padding:6,flexShrink:0,cursor:'pointer'}}><Text style={{fontFamily:mono,fontSize:12,color:path()==='/'?C.text:C.amber}}>/</Text></View>
                <If when={collapsedCrumbs().hidden.length>0}>
                  <Text pointerEvents="none" style={{fontFamily:mono,fontSize:12,color:C.muted}}>›</Text>
                  <View ref={node=>{breadcrumbOverflow=node;}} desktop={{windowDrag:false}} role="button" accessibilityLabel="Show hidden path segments" accessibilityState={{disabled:busy() || browseBusy()}} onPress={()=>void showBreadcrumbs()} class="hover:brightness-125" style={{paddingHorizontal:6,paddingVertical:4,borderRadius:3,flexShrink:0,cursor:'pointer'}}><Text style={{fontFamily:mono,fontSize:14,color:C.amber}}>…</Text></View>
                </If>
                <Each of={collapsedCrumbs().tail}>{n=><View style={{flexDirection:'row',alignItems:'center',gap:3,flexShrink:1,minWidth:0}}>
                  <Text pointerEvents="none" style={{fontFamily:mono,fontSize:12,color:C.muted,flexShrink:0}}>›</Text>
                  <View desktop={{windowDrag:false}} role="button" accessibilityLabel={`Go to ${n().path}`} accessibilityState={{disabled:busy() || browseBusy() || n().path===current()?.path}} onPress={()=>{if(n().path!==current()?.path)navigate(n().path);}} class="hover:brightness-125" style={{padding:6,borderRadius:3,flexShrink:1,minWidth:0,cursor:n().path===current()?.path?'default':'pointer'}}>
                    <Text pointerEvents="none" numberOfLines={1} ellipsizeMode="middle" style={{fontFamily:mono,fontSize:12,maxWidth:180,flexShrink:1,color:n().path===current()?.path?C.text:C.amber}}>{n().name}</Text>
                  </View>
                </View>}</Each>
              </View>
            </If>
            <If when={editingPath()}><Input ref={node=>{pathField=node;node.focus();}} accessibilityLabel="Folder path" value={draftPath()} onChange={v=>draftPath.set(v)} onSubmit={()=>{editingPath.set(false);void scan(draftPath());}} autoCorrect={false} autoCapitalize="none" style={{fontFamily:mono,color:C.text,fontSize:12,padding:8,backgroundColor:'#09090b'}}/></If>
          </View>
          <View style={{width:220,flexShrink:0,height:28,justifyContent:'center'}}>
            <Text numberOfLines={1} style={{fontFamily:mono,fontSize:11,color:C.muted,textAlign:'right'}}>{current()?`${formatSize(current()!.size)} · ${current()!.files.toLocaleString()} files`:''}</Text>
          </View>
          <Input ref={node=>{filterField=node;}} value={query()} onChange={v=>batch(()=>{query.set(v);page.set(0);})} placeholder="Filter names…" style={{placeholderColor:C.muted,fontFamily:mono,width:130,flexShrink:0,fontSize:11,color:C.text,padding:6,backgroundColor:'#18181c',borderRadius:4}}/>
        </View>
        <Scroll horizontal showsIndicator={false} style={{height:20,flexGrow:0,flexShrink:0}}><View style={{flexDirection:'row',gap:13,alignItems:'center'}}><Each of={categories}>{category=><View style={{flexDirection:'row',alignItems:'center',gap:4}}><View style={{width:8,height:8,borderRadius:1,backgroundColor:category().color}}/><Text style={{fontFamily:mono,fontSize:10,color:C.muted}}>{category().label}</Text></View>}</Each></View></Scroll>
        <View testID="treemap" class="hover:opacity-100" onHoverIn={()=>{pointerInMap.set(true);pathField?.blur();filterField?.blur();}} onHoverOut={()=>{pointerInMap.set(false);hovered.set(new Map());}} pinch={{scale:cameraScale}} pan={{x:cameraX,y:cameraY}} onPinchEnd={()=>{void cameraScale.getValue().then(endPinch);}} onLayout={e=>frame.set({width:e.width,height:e.height})} style={{flex:1,overflow:'hidden',backgroundColor:'#050506',borderRadius:3}}>
          <View style={{width:frame().width,height:frame().height,transform:cameraTransform()}}><Each of={tiles()} key={tile=>tile.node.path}>{tile=><MapTile tile={tile()} selected={selection()?.path===tile().node.path} marked={marked().some(n=>n.path===tile().node.path || tile().node.path.startsWith(n.path+'/'))} dimmed={!!selection() && selection()?.path!==current()?.path && selection()?.path!==tile().node.path} metric={listingMetric()} levels={detailLevels()} zoom={zoom()} hover={()=>hoverNode(tile().node,true)} leave={()=>hoverNode(tile().node,false)} choose={()=>selectOrEnter(tile().node)} open={()=>enter(tile().node)}/>}</Each>
          </View>
          <If when={!tiles().length}><View pointerEvents="none" style={{position:'absolute',left:0,right:0,top:0,bottom:0}}><EmptyMap busy={busy()}/></View></If>
        </View>
      </View>
      <View style={{width:284,backgroundColor:C.panel,borderLeftWidth:1,borderColor:C.line}}>
        <Scroll style={{flex:1}}>
          <View style={{padding:16,gap:12,opacity:panelReveal,transform:[{translateX:panelReveal.interpolate({inputRange:[0,1],outputRange:[8,0]})}]}}>
            <Text style={{fontFamily:mono,fontSize:10,letterSpacing:1,color:C.muted}}>{review()?(removal()==='empty'?'EMPTY TRASH':'REVIEW REMOVAL'):'SELECTION'}</Text>
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
              <Text style={{fontFamily:mono,fontSize:15,color:C.text}}>{removal()==='empty'?'Empty the system Trash?':removal()==='delete'?'Permanently delete these items?':'Move these items to Trash?'}</Text>
              <Text style={{fontFamily:mono,fontSize:11,color:C.muted}}>{removal()==='trash'?'Recoverable from the system Trash.': 'This permanently removes files and cannot be undone.'}</Text>
              <Each of={removal()==='empty'?[]:reviewed()}>{node=><View style={{gap:7,paddingVertical:6}}><Text style={{fontFamily:mono,fontSize:11,color:C.text}}>{node().path}</Text><Button label={`Exclude · ${formatSize(node().size)}`} action={()=>{if(!busy()){reviewed.set(reviewed().filter(n=>n.path!==node().path));}}}/></View>}</Each>
              <Button label={busy()?'Working…':removal()==='empty'?'Confirm empty Trash':removal()==='delete'?'Confirm permanent deletion':'Confirm move to Trash'} action={()=>void commit()} danger={removal()!=='trash'} active={removal()==='trash'} disabled={busy() || browseBusy() || (removal()!=='empty' && !reviewed().length)}/>
              <If when={removal()==='trash'}><Button label="Delete permanently" danger disabled={busy() || browseBusy() || !reviewed().length} action={()=>removal.set('delete')}/></If>
              <Button label="Back to explorer" disabled={busy()} action={()=>review.set(false)}/>
            </If>
            <View style={{height:1,backgroundColor:C.line}}/>
            <Text style={{fontFamily:mono,fontSize:12,color:C.text}}>{marked().length} marked · {formatSize(markedBytes())}</Text>
            <If when={!review()}><Button label="Review marked items →" disabled={!marked().length || busy() || browseBusy()} action={()=>beginReview('trash')} active={marked().length>0}/></If>
            <If when={!review()}><Button label="Delete marked permanently" danger disabled={!marked().length || busy() || browseBusy()} action={()=>beginReview('delete')}/><Button label="Empty Trash" disabled={busy() || browseBusy()} action={()=>beginReview('empty')}/></If>
            <If when={!review()}>
              <View style={{height:1,backgroundColor:C.line}}/>
              <View style={{flexDirection:'row',alignItems:'center',gap:5}}><Text style={{fontFamily:mono,flex:1,fontSize:10,letterSpacing:0.8,color:C.muted}}>CONTENTS · {listing()?.count || 0} ITEMS</Text><Text style={{fontFamily:mono,fontSize:10,color:C.muted}}>{listing()?.count?`${listing()!.offset+1}–${Math.min(listing()!.offset+listing()!.pageSize,listing()!.count)}`:''}</Text></View>
              <View style={{flexDirection:'row',justifyContent:'space-between'}}><Button label="‹ Previous" action={()=>changePage(page()-(listing()?.pageSize || 80))}/><Button label="Next ›" action={()=>{const v=listing();if(v && v.offset+v.pageSize<v.count)changePage(v.offset+v.pageSize);}}/></View>
              <Each of={children()} key={node=>node.path}>{node=><View onPress={()=>selectOrEnter(node())} class="hover:brightness-125" style={{paddingVertical:7,gap:5,cursor:'pointer',borderBottomWidth:1,borderColor:'#242428'}}><View style={{flexDirection:'row',gap:6,alignItems:'center'}}><View style={{width:3,height:13,backgroundColor:kind(node()).color}}/><Text numberOfLines={1} style={{fontFamily:mono,flex:1,fontSize:11,color:selection()?.path===node().path?C.amber:C.text}}>{marked().some(n=>n.path===node().path)?'✓ ':''}{node().directory?'▸ ':''}{node().name}</Text><Text style={{fontFamily:mono,fontSize:10,color:C.muted}}>{listingMetric()==='files'?String(node().files):formatSize(node()[listingMetric()])}</Text></View><View style={{height:2,backgroundColor:'#101012'}}><View style={{height:2,width:`${100*node()[listingMetric()]/(current()?.[listingMetric()] || 1)}%`,backgroundColor:kind(node()).color}}/></View></View>}</Each>
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
    <View testID="statusbar" style={{height:28,flexShrink:0,flexDirection:'row',gap:16,alignItems:'center',paddingHorizontal:12,borderTopWidth:1,borderColor:C.line,backgroundColor:'#101012'}}>
      <If when={scanProgress()}>
        <View pointerEvents="none" testID="scan-progress" style={{flex:1,minWidth:0,flexDirection:'row',alignItems:'center',gap:12}}>
          <View style={{flexDirection:'row',gap:16,alignItems:'center'}}>
            <Text style={{fontFamily:mono,fontSize:10,color:C.amber}}>Scanning folders</Text>
            <Text numberOfLines={1} style={{fontFamily:mono,fontSize:10,color:C.muted}}>{scanProgress()!.directories.toLocaleString()} folders · {scanProgress()!.files.toLocaleString()} files · {formatSize(scanProgress()!.bytes)} found · {scanSeconds()}s elapsed{scanProgress()!.errors?` · ${scanProgress()!.errors} skipped`:''}</Text>
          </View>
          <Text numberOfLines={1} ellipsizeMode="middle" style={{fontFamily:mono,fontSize:10,color:C.text,flex:1}}>{scanProgress()!.path}</Text>
        </View>
      </If>
      <If when={!scanProgress()}><Text style={{fontFamily:mono,fontSize:10,color:C.muted}}>Pinch / ⌘± Zoom   ⌘0 Fit   Click selected Enter   Space Mark   ⌥↑ Parent   ⌘R Rescan   Esc Close review</Text><Text numberOfLines={1} style={{fontFamily:mono,flex:1,fontSize:10,color:'#85858f',textAlign:'right'}}>{message()}</Text></If>
    </View>
  </View>;
}
mount(App);
