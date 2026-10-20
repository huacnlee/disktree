import { cleanup, effect, Text, View, type JSX } from 'nsh';
import { Animated, createAnimatedValue } from 'nsh/animated';
import { Platform } from 'nsh/device';
import { reduceMotion } from 'nsh/accessibility';
import { Canvas } from 'nsh/canvas';
import { formatSize, tileColor, type Tile } from './disk';

export const mono=Platform.OS==='macos'?'Menlo':'monospace';
export const palette = { bg:'#070708',panel:'#101012',line:'#29292d',text:'#ededf0',muted:'#85858f',amber:'#65f193' };
export const finish = {boxShadow:'inset 0 1px 0 rgba(255,255,255,0.045)'};
export function Appear(p: { children: JSX.Element; delay?: number; flex?: number }) {
  const value = createAnimatedValue(0);
  effect(() => {
    if (reduceMotion()) value.set(1);
    else void value.timing(1, {duration:600,delay:p.delay || 0,easing:[0.16,1,0.3,1]});
  });
  return <View style={{flex:p.flex,opacity:value,transform:[{translateY:value.interpolate({inputRange:[0,1],outputRange:[16,0]})}]}}>{p.children}</View>;
}
export function Button(p: { label: string; action: () => void; active?: boolean; disabled?:boolean; danger?:boolean; icon?:'scan'|'parent'|'edit'; accessibilityLabel?:string }) {
  const press = createAnimatedValue(1);
  const set = (v:number) => { if(reduceMotion())press.set(v);else void press.spring(v,{stiffness:420,damping:24}); };
  return <View role="button" accessibilityLabel={p.accessibilityLabel || p.label} accessibilityState={{disabled:!!p.disabled}} onPress={()=>{if(!p.disabled)p.action();}} onPressDown={()=>{if(!p.disabled)set(0.955);}} onPressUp={()=>set(1)} class="hover:brightness-125" style={{paddingHorizontal:p.icon?10:14,paddingVertical:10,alignItems:'center',justifyContent:'center',borderRadius:4,borderCurve:'continuous',borderWidth:1,borderColor:p.danger?'#a65a50':p.active?'#68f296':'#36363d',backgroundColor:p.danger?'#38201e':p.active?'#203329':'#202024',boxShadow:'inset 0 1px 0 rgba(255,255,255,0.09), inset 0 -1px 0 rgba(0,0,0,0.35), 0 1px 2px rgba(0,0,0,0.45)',opacity:p.disabled?0.45:1,cursor:p.disabled?'default':'pointer',transform:[{scale:press}]}}>
    <If when={!!p.icon}><Canvas pointerEvents="none" style={{width:16,height:16}} draw={ctx=>{
      ctx.strokeStyle=palette.text;ctx.lineWidth=1.6;ctx.lineCap='round';ctx.lineJoin='round';ctx.beginPath();
      if(p.icon==='scan'){ctx.arc(8,8,5.5,0.1,Math.PI*1.7);ctx.moveTo(11,1);ctx.lineTo(12,4.5);ctx.lineTo(8.5,4.5);}
      else if(p.icon==='parent'){ctx.moveTo(8,14);ctx.lineTo(8,2);ctx.moveTo(3,7);ctx.lineTo(8,2);ctx.lineTo(13,7);}
      else {ctx.moveTo(3,10);ctx.lineTo(10,3);ctx.lineTo(13,6);ctx.lineTo(6,13);ctx.lineTo(2,14);ctx.lineTo(3,10);ctx.moveTo(8,5);ctx.lineTo(11,8);}
      ctx.stroke();
    }}/></If>
    <If when={!p.icon}><Text style={{fontFamily:mono,fontSize:12,fontWeight:'600',color:p.danger?'#ffc6bd':p.active?'#fff':palette.text}}>{p.label}</Text></If>
  </View>;
}
export function Capacity(p: {total:number;free:number;busy:boolean}) {
  const fill = createAnimatedValue(0);
  const spin = createAnimatedValue(0);
  effect(()=>{const to=p.total?1-p.free/p.total:0;if(reduceMotion())fill.set(to);else void fill.timing(to,{duration:1100,easing:[0.16,1,0.3,1]});});
  effect(()=>{
    if(!p.busy || reduceMotion()){spin.set(0);return;}
    const loop=Animated.loop(Animated.timing(spin,{toValue:Math.PI*2,duration:2400,easing:'linear'}));
    void loop.start();cleanup(()=>loop.stop());
  });
  return <View style={{width:142,height:142,alignItems:'center',justifyContent:'center'}}>
    <Canvas pointerEvents="none" style={{position:'absolute',left:0,top:0,width:142,height:142}} draw={ctx=>{
      ctx.lineWidth=1;
      for(let i=0;i<60;i++){const a=i*Math.PI/30-Math.PI/2;ctx.strokeStyle=i%5===0?'#777780':'#252529';ctx.beginPath();ctx.moveTo(71+Math.cos(a)*64,71+Math.sin(a)*64);ctx.lineTo(71+Math.cos(a)*(i%5===0?59:62),71+Math.sin(a)*(i%5===0?59:62));ctx.stroke();}
      ctx.lineWidth=6;ctx.lineCap='round';ctx.strokeStyle='#29292e';ctx.beginPath();ctx.arc(71,71,50,-Math.PI/2,Math.PI*1.5);ctx.stroke();
      ctx.strokeStyle='#65f193';ctx.beginPath();ctx.arc(71,71,50,-Math.PI/2,fill.mul(Math.PI*2).sub(Math.PI/2));ctx.stroke();
      if(p.busy){ctx.lineWidth=2;ctx.strokeStyle='#e8e8ed';ctx.beginPath();ctx.arc(71,71,55,spin,spin.add(0.6));ctx.stroke();}
    }}/>
    <Text style={{fontFamily:mono,fontSize:25,fontWeight:'600',letterSpacing:-1,color:palette.text,fontVariant:['tabular-nums']}}>{p.total?`${Math.round(100*(1-p.free/p.total))}%`:'—'}</Text>
    <Text style={{fontFamily:mono,fontSize:9,color:palette.muted,letterSpacing:1.5}}>OCCUPIED</Text>
  </View>;
}
export function MapTile(p: {tile:Tile;selected:boolean;dimmed:boolean;marked:boolean;metric:string;levels:number;zoom:number;hover:()=>void;leave:()=>void;choose:()=>void;open:()=>void}) {
  cleanup(()=>p.leave());
  const ink=()=>p.marked?'#ffe6ab':'#e2e5ec';
  const tint=(alpha:number)=>{const hex=tileColor(p.tile.node,p.tile.depth);return `rgba(${parseInt(hex.slice(1,3),16)},${parseInt(hex.slice(3,5),16)},${parseInt(hex.slice(5,7),16)},${alpha})`;};
  return <View role="button" accessibilityLabel={p.tile.node.name} onHoverIn={()=>p.hover()} onHoverOut={()=>p.leave()} onPress={p.choose} class="hover:brightness-125" style={{position:'absolute',left:p.tile.x+1/p.zoom,top:p.tile.y+1/p.zoom,width:Math.max(0,p.tile.width-2/p.zoom),height:Math.max(0,p.tile.height-2/p.zoom),borderRadius:3,borderCurve:'continuous',overflow:'hidden',backgroundColor:p.marked?'rgba(210,145,35,0.2)':tint(0.09),backgroundImage:`linear-gradient(155deg, ${p.marked?'rgba(255,208,105,0.08)':tint(0.07)}, rgba(0,0,0,0.12))`,borderWidth:(p.marked?2:1)/p.zoom,borderColor:p.marked?'#ffd36c':tint(p.selected?0.95:0.38),padding:5/p.zoom,opacity:p.dimmed?0.94:1,cursor:'pointer'}}>
    <If when={p.marked}><Canvas pointerEvents="none" style={{position:'absolute',left:0,top:0,right:0,bottom:0}} draw={(ctx,size)=>{ctx.strokeStyle='rgba(255,221,142,0.25)';ctx.lineWidth=1/p.zoom;ctx.beginPath();for(let x=-size.height;x<size.width;x+=10/p.zoom){ctx.moveTo(x,size.height);ctx.lineTo(x+size.height,0);}ctx.stroke();}}/></If>
    <If when={p.marked && p.tile.width*p.zoom>110 && p.tile.height*p.zoom>50}><View pointerEvents="none" style={{position:'absolute',right:5/p.zoom,bottom:5/p.zoom,paddingHorizontal:5/p.zoom,paddingVertical:2/p.zoom,borderRadius:3/p.zoom,backgroundColor:'#ffd36c',opacity:1}}><Text style={{fontFamily:mono,fontSize:9/p.zoom,fontWeight:'700',color:'#382305'}}>✓ MARKED</Text></View></If>
    <If when={p.tile.width*p.zoom>70 && p.tile.height*p.zoom>26}><View style={{flexDirection:'row',gap:5,alignItems:'center'}}><Text numberOfLines={1} style={{fontFamily:mono,flex:1,fontSize:11/p.zoom,fontWeight:'600',color:ink()}}>{p.marked?'✓ ':''}{p.tile.node.name}</Text><If when={p.tile.node.directory && p.tile.width*p.zoom>140}><Text onPress={p.open} style={{fontFamily:mono,fontSize:13/p.zoom,color:ink()}}>↗</Text></If></View></If>
    <If when={p.tile.width*p.zoom>90 && p.tile.height*p.zoom>70 && (p.tile.depth===p.levels || !p.tile.node.children.length)}><Text style={{fontFamily:mono,marginTop:5/p.zoom,fontSize:(p.tile.width*p.zoom>200?22:13)/p.zoom,fontWeight:'500',fontVariant:['tabular-nums'],letterSpacing:-0.5,color:ink()}}>{p.metric==='files'?`${p.tile.node.files} files`:formatSize(p.metric==='apparent'?p.tile.node.apparent:p.tile.node.size)}</Text></If>
  </View>;
}
export function EmptyMap(p:{busy:boolean}) {
  const scan=createAnimatedValue(0);
  effect(()=>{
    if(!p.busy || reduceMotion()){scan.set(0);return;}
    const loop=Animated.loop(Animated.sequence([Animated.timing(scan,{toValue:1,duration:1600,easing:'easeInOut'}),Animated.timing(scan,{toValue:0,duration:1600,easing:'easeInOut'})]));void loop.start();cleanup(()=>loop.stop());
  });
  return <View style={{flex:1,alignItems:'center',justifyContent:'center',gap:20}}>
    <View style={{width:220,height:144,transform:[{rotate:'-6deg'}],backgroundColor:'#17171b',borderRadius:14,padding:7,borderWidth:1,borderColor:'#393940',boxShadow:'inset 0 1px 0 rgba(255,255,255,0.08), 0 20px 50px rgba(0,0,0,0.4)',flexDirection:'row',gap:5}}>
      <View style={{flex:1.4,backgroundColor:'#303039',borderRadius:7,padding:12,justifyContent:'flex-end'}}><View style={{width:26,height:3,backgroundColor:'#52b976',borderRadius:2}}/></View>
      <View style={{flex:1,gap:5}}><View style={{flex:1,backgroundColor:'#26262e',borderRadius:7}}/><View style={{flex:1.3,backgroundColor:'#202026',borderRadius:7}}/></View>
      <View pointerEvents="none" style={{position:'absolute',left:0,top:0,bottom:0,width:2,backgroundColor:'#65f193',opacity:p.busy?0.8:0,transform:[{translateX:scan.mul(218)}],boxShadow:'0 0 20px #65f193'}}/>
    </View>
    <View style={{alignItems:'center',gap:8}}><Text style={{fontFamily:mono,fontSize:24,fontWeight:'500',letterSpacing:-0.7,color:palette.text}}>{p.busy?'Mapping your space':'See the shape of your storage.'}</Text><Text style={{fontFamily:mono,fontSize:12,color:palette.muted}}>{p.busy?'Reading folders and measuring their footprint…':'Scan a folder. Find the weight. Make room.'}</Text></View>
  </View>;
}

export function MetricControl(p:{value:'size'|'apparent'|'files';change:(value:'size'|'apparent'|'files')=>void}) {
  const offset=createAnimatedValue(0);
  const options=[{key:'size' as const,label:'Disk size'},{key:'apparent' as const,label:'Apparent'},{key:'files' as const,label:'Files'}];
  effect(()=>{const target=options.findIndex(v=>v.key===p.value)*76;if(reduceMotion())offset.set(target);else void offset.spring(target,{stiffness:350,damping:32});});
  return <View style={{flexDirection:'row',padding:3,borderRadius:4,backgroundColor:'#09090b',borderWidth:1,borderColor:'#303036',backgroundImage:'linear-gradient(180deg, rgba(255,255,255,0.025), transparent)',boxShadow:'inset 0 1px 0 rgba(255,255,255,0.035), 0 2px 4px rgba(0,0,0,0.2)'}}>
    <View pointerEvents="none" style={{position:'absolute',left:3,top:3,bottom:3,width:76,borderRadius:2,backgroundColor:'#303036',borderWidth:1,borderColor:'#484850',boxShadow:'inset 0 1px 0 rgba(255,255,255,0.08), inset 0 -1px 0 rgba(0,0,0,0.3), 0 1px 2px rgba(0,0,0,0.4)',transform:[{translateX:offset}]}}/>
    <Each of={options}>{v=><View role="tab" accessibilityLabel={v().label} accessibilityState={{selected:p.value===v().key}} onPress={()=>p.change(v().key)} style={{width:76,paddingVertical:8,alignItems:'center',cursor:'pointer'}}><Text style={{fontFamily:mono,fontSize:11,fontWeight:'500',color:p.value===v().key?'#f1f1f4':'#93939f'}}>{v().label}</Text></View>}</Each>
  </View>;
}

/** Original capacity-ring mark; shared geometry with assets/icon.svg. */
export function AppMark() {
  return <Canvas pointerEvents="none" style={{width:24,height:24}} draw={ctx=>{
    ctx.fillStyle='#101012';ctx.beginPath();ctx.roundRect(0,0,24,24,6);ctx.fill();
    ctx.strokeStyle='#65f193';ctx.lineWidth=1.8;ctx.lineCap='round';ctx.beginPath();ctx.arc(12,12,6.6,-0.7,Math.PI*2);ctx.stroke();
    ctx.strokeStyle='#baffcb';ctx.lineWidth=1.1;ctx.beginPath();ctx.moveTo(9.4,9.4);ctx.lineTo(13.8,9.4);ctx.moveTo(9.4,12);ctx.lineTo(15.2,12);ctx.moveTo(9.4,14.6);ctx.lineTo(12.6,14.6);ctx.stroke();
    ctx.fillStyle='#baffcb';ctx.beginPath();ctx.arc(18.6,8.5,1,0,Math.PI*2);ctx.fill();
  }}/>;
}
