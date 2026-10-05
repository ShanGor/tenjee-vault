import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { usePreferences } from "../i18n";
import { ui } from "../i18n/ui";
import { useMobileDismiss } from "./useMobileDismiss";
import { flushPageSave } from "../modules/notes/pageSave";
import { reconcileReminders } from "./mobileReminders";
import { formatExchangeError } from "./exchangeError";

type Summary = { records:number; deletions:number; conflicts:number; blobs:number };
type Peer = { session:string; label:string; platform:string; addresses:string[] };
type NetworkOption = { name:string; vpn:boolean; addresses:string[] };
type Result = { applied:number; unchanged:number; conflicts:number; pendingGroups:number; pendingBlobs:number; pendingLocalChanges:boolean };
type Status = { phase:string; code:string|null; expiresIn:number; attemptsLeft:number; addresses:string[]; peers:Peer[]; peer:Peer|null; localSummary:Summary|null; peerSummary:Summary|null; result:Result|null; message:string; transferredRecords:number; totalRecords:number; attachmentBytes:number };
const idle:Status = {phase:"idle",code:null,expiresIn:0,attemptsLeft:0,addresses:[],peers:[],peer:null,localSummary:null,peerSummary:null,result:null,message:"",transferredRecords:0,totalRecords:0,attachmentBytes:0};

export function ExchangePage() {
  const { formatError }=usePreferences();
  const [available,setAvailable]=useState<boolean|null>(null);
  const [status,setStatus]=useState<Status>(idle);
  const [label,setLabel]=useState(()=>{try{return localStorage.getItem("tenjee-device-label") ?? "Tenjee Vault";}catch{return "Tenjee Vault";}});
  const [networks,setNetworks]=useState<NetworkOption[]>([]);
  const [network,setNetwork]=useState("");
  const [port,setPort]=useState(()=>{try{const saved=localStorage.getItem("tenjee-exchange-port") ?? "";return /^\d+$/.test(saved) && Number(saved)>=1 && Number(saved)<=65535?saved:"";}catch{return "";}});
  const [address,setAddress]=useState("");const [code,setCode]=useState("");
  const [busy,setBusy]=useState(false);const [error,setError]=useState("");
  const [conflicts,setConflicts]=useState<Conflict[]>([]);
  const [copyChoice,setCopyChoice]=useState<{conflict:Conflict;variant:string}|null>(null);
  const [copyPassword,setCopyPassword]=useState("");
  useMobileDismiss(()=>{if(!busy){setCopyChoice(null);setCopyPassword("");}},!!copyChoice);
  const active=["discovering","pairing","negotiating","approval","exchanging"].includes(status.phase);
  const validPort=port==="" || (/^\d+$/.test(port) && Number(port)>=1 && Number(port)<=65535);
  const phaseLabels:Record<string,string>={idle:ui("交换未开启"),discovering:ui("正在寻找局域网设备"),pairing:ui("正在验证配对码"),negotiating:ui("正在准备交换范围"),approval:ui("等待双方确认"),exchanging:ui("正在交换数据"),finished:ui("交换已完成"),failed:ui("交换未完成"),stopped:ui("交换已停止")};
  const exchangeError=(error:unknown)=>formatExchangeError(error,formatError);
  const refreshConflicts=()=>invoke<Conflict[]>("exchange_conflicts_cmd").then(setConflicts);
  const refreshNetworks=()=>invoke<NetworkOption[]>("exchange_networks_cmd").then(setNetworks);
  useEffect(()=>{
    let alive=true;let refreshing=false;
    const refresh=async()=>{
      if(refreshing)return;refreshing=true;
      try {const next=await invoke<Status>("exchange_status_cmd");if(alive)setStatus(next);}
      catch(reason){if(alive)setError(exchangeError(reason));}finally{refreshing=false;}
    };
    void invoke<boolean>("exchange_available_cmd").then(value=>{if(alive)setAvailable(value);});
    void invoke<NetworkOption[]>("exchange_networks_cmd").then(value=>{if(alive)setNetworks(value);}).catch(reason=>{if(alive)setError(exchangeError(reason));});
    void refresh();void refreshConflicts().catch(reason=>setError(exchangeError(reason)));
    const timer=window.setInterval(()=>void refresh(),750);
    const suspended=()=>{if(document.hidden){setCode("");void invoke("exchange_stop_cmd").catch(()=>undefined);}};
    document.addEventListener("visibilitychange",suspended);
    return()=>{alive=false;clearInterval(timer);document.removeEventListener("visibilitychange",suspended);void invoke("exchange_stop_cmd").catch(()=>undefined);};
  },[formatError]);
  useEffect(()=>{if(status.phase==="finished"){void refreshConflicts().catch(reason=>setError(exchangeError(reason)));void reconcileReminders().catch(()=>undefined);}},[status.phase]);
  async function action(work:()=>Promise<unknown>) {setBusy(true);setError("");try{await work();setStatus(await invoke<Status>("exchange_status_cmd"));}catch(reason){setError(exchangeError(reason));}finally{setBusy(false);}}
  async function enter(){await flushPageSave();try{localStorage.setItem("tenjee-device-label",label.trim());localStorage.setItem("tenjee-exchange-port",port);}catch{/* optional device preferences */}setStatus(await invoke<Status>("exchange_enter_cmd",{label:label.trim(),interface:network || null,port:port===""?null:Number(port)}));}
  async function resolve(conflict:Conflict,variant:string){await action(async()=>{await invoke("exchange_resolve_cmd",{store:conflict.store,entity:conflict.entity,key:conflict.key,variant});await refreshConflicts();});}
  return <main className="h-full overflow-y-auto p-4 sm:p-6"><div className="mx-auto max-w-3xl space-y-5">
    <h1 className="text-xl font-semibold">{ui("寻找其他设备并交换")}</h1>
    <p className="text-sm text-neutral-600">{ui("将两台设备连接到可互通的局域网或 VPN（如 Tailscale），分别开启交换模式。仅在此页面保持前台时交换，离开或锁屏后需要重新配对。")}</p>
    {available===false && <p role="status">{ui("设备交换正在开发验证，仅开发构建可用。")}</p>}
    {error && <p role="alert" className="rounded border border-red-300 p-3">{error}</p>}
    <section className="space-y-3 rounded-lg border p-4">
      <label className="block">{ui("本机名称")}<input className="mt-1 block w-full rounded border p-2" maxLength={64} value={label} disabled={active} onChange={event=>setLabel(event.target.value)}/></label>
      <label className="block">{ui("交换网络")}<select className="mt-1 block w-full rounded border p-2" value={network} disabled={active || busy} onChange={event=>setNetwork(event.target.value)}><option value="">{ui("自动选择局域网")}</option>{networks.map(option=><option key={option.name} value={option.name}>{option.name}{option.vpn?" · VPN":""} · {option.addresses.join(", ")}</option>)}</select></label>
      <p className="text-sm text-neutral-600">{ui("使用 VPN 时，请在两台设备上选择对应网络。VPN 必须允许设备之间连接；附近设备发现仅用于局域网。")}</p>
      {!active && <button className="rounded border px-3 py-2" disabled={busy} onClick={()=>void action(refreshNetworks)}>{ui("刷新网络")}</button>}
      <label className="block">{ui("交换端口（可选）")}<input className="mt-1 block w-full rounded border p-2" inputMode="numeric" placeholder={ui("自动分配")} maxLength={5} value={port} disabled={active || busy} aria-invalid={!validPort} onChange={event=>setPort(event.target.value)}/></label>
      <p className="text-sm text-neutral-600">{ui("留空自动分配，或输入 1–65535 的固定端口，方便使用主机名连接。两台设备的端口可不同，请使用对方显示的端口。")}</p>
      <p role="status" aria-live="polite">{phaseLabels[status.phase] ?? status.phase}</p>
      {status.phase==="exchanging" && <p aria-live="polite" className="text-sm">{ui("记录 {p0} / {p1}；附件传输 {p2} MiB",{p0:String(status.transferredRecords),p1:String(status.totalRecords),p2:(status.attachmentBytes/1048576).toFixed(1)})}</p>}
      {status.message && <p className="break-words text-sm">{exchangeError(status.message)}</p>}
      {!active ? <button className="rounded border px-4 py-2" disabled={busy || !available || !label.trim() || !validPort || (!!network && !networks.some(option=>option.name===network))} onClick={()=>void action(enter)}>{ui("开启交换模式")}</button> : <button className="rounded border px-4 py-2" disabled={busy} onClick={()=>void action(()=>invoke("exchange_stop_cmd"))}>{ui("停止交换")}</button>}
      {status.code && <><p>{ui("让另一台设备输入此配对码")}</p><div className="break-all font-mono text-3xl tracking-widest" aria-label={ui("配对码")}>{status.code}</div><p className="text-sm">{ui("剩余 {p0} 秒；还有 {p1} 次尝试。",{p0:String(status.expiresIn),p1:String(status.attemptsLeft)})}</p><p className="text-sm">{ui("设备名称不代表身份，请当面核对配对码。")}</p></>}
      {status.addresses.length>0 && status.phase==="discovering" && <details open><summary>{ui("本机交换地址")}</summary>{status.addresses.map(value=><p className="break-all font-mono text-sm" key={value}>{value}</p>)}</details>}
    </section>
    {status.phase==="discovering" && <section className="space-y-3 rounded-lg border p-4"><h2 className="font-medium">{ui("选择设备或输入主机名或 IP")}</h2>
      {status.peers.length===0 && <p className="text-sm">{ui("暂未发现设备。可手动输入对方的主机名或 IP 和端口；Tailscale 可使用 MagicDNS 名称。")}</p>}
      {status.peers.map(peer=><button className={`block w-full rounded border p-3 text-left ${address===peer.addresses[0]?"border-blue-500":""}`} key={peer.session+peer.addresses.join()} onClick={()=>setAddress(peer.addresses[0])}>{peer.label} · {peer.platform}<span className="block break-all text-xs">{peer.addresses[0]}</span></button>)}
      <label className="block">{ui("对方主机名或 IP 和端口")}<input className="mt-1 block w-full rounded border p-2" placeholder="my-laptop:49152" value={address} onChange={event=>setAddress(event.target.value)} autoCapitalize="none" spellCheck={false}/></label>
      <label className="block">{ui("对方配对码")}<input className="mt-1 block w-full rounded border p-2 font-mono" inputMode="numeric" autoComplete="off" maxLength={8} value={code} onChange={event=>setCode(event.target.value.replace(/\D/g,""))}/></label>
      <button className="rounded border px-4 py-2" disabled={busy || code.length!==8 || !address.trim()} onClick={()=>void action(async()=>{const entered=code;setCode("");await invoke("exchange_connect_cmd",{address:address.trim(),code:entered});})}>{ui("验证并查看交换范围")}</button>
    </section>}
    {status.phase==="approval" && <section className="space-y-3 rounded-lg border p-4"><h2 className="font-medium">{ui("确认与 {p0} 交换整个工作区",{p0:status.peer?.label ?? ""})}</h2><p className="text-sm">{ui("包括笔记、任务、日历、附件和删除记录。受保护内容仍需在本机输入其保护密码解锁。备份设置、通知权限和本机偏好不会交换。")}</p>
      <SummaryView title={ui("本机")} summary={status.localSummary}/><SummaryView title={ui("对方")} summary={status.peerSummary}/>
      <button className="rounded border px-4 py-2" disabled={busy} onClick={()=>void action(()=>invoke("exchange_approve_cmd"))}>{ui("确认交换整个工作区")}</button>
    </section>}
    {status.result && <section className="space-y-2 rounded-lg border p-4"><h2 className="font-medium">{ui("交换结果")}</h2><p>{ui("已应用 {p0} 项；无需更新 {p1} 项；冲突 {p2} 项。",{p0:String(status.result.applied),p1:String(status.result.unchanged),p2:String(status.result.conflicts)})}</p>
      {status.result.pendingGroups>0 && <p>{ui("{p0} 个依赖组仍待处理。请先解锁迁移旧标题或解决保护冲突，再重新配对。",{p0:String(status.result.pendingGroups)})}</p>}
      {status.result.pendingGroups>0 && <button className="rounded border p-2" disabled={busy || active} onClick={()=>{if(window.confirm(ui("清除尚未提交的交换文件？已提交的数据和原设备上的修改会保留，之后可重新配对。")))void action(()=>invoke("exchange_discard_pending_cmd"));}}>{ui("清除待处理的交换文件")}</button>}
      {status.result.pendingLocalChanges && <p>{ui("交换期间产生了新的本机修改，下次交换会继续同步。")}</p>}
    </section>}
    {copyChoice && <section className="space-y-3 rounded-lg border p-4" role="dialog" aria-label={ui("保留两个版本")}><h2 className="font-medium">{ui("保留两个版本")}</h2><p className="text-sm">{ui("所选版本会复制为新项目，原项目保留当前版本。页面和任务子树中的内部引用随副本更新。受保护子树需先在本机解锁。")}</p>
      {copyChoice.conflict.entity==="protected-domain" && <label className="block">{ui("所选受保护版本的密码（仅在本机使用）")}<input type="password" autoComplete="off" className="mt-1 block w-full rounded border p-2" value={copyPassword} onChange={event=>setCopyPassword(event.target.value)}/></label>}
      <div className="flex gap-3"><button className="rounded border p-2" disabled={busy || (copyChoice.conflict.entity==="protected-domain" && !copyPassword)} onClick={()=>void action(async()=>{const choice=copyChoice;const password=copyPassword;setCopyPassword("");await invoke("exchange_keep_both_cmd",{store:choice.conflict.store,entity:choice.conflict.entity,key:choice.conflict.key,variant:choice.variant,password:password || null});setCopyChoice(null);await refreshConflicts();})}>{ui("保留两个版本")}</button><button className="rounded border p-2" disabled={busy} onClick={()=>{setCopyChoice(null);setCopyPassword("");}}>{ui("取消")}</button></div>
    </section>}
    {conflicts.length>0 && <section className="space-y-3"><h2 className="font-medium">{ui("待解决的交换冲突")}</h2><p className="text-sm">{ui("选择要保留的版本。其他版本在解决前持续保留。受保护冲突只显示密文元数据。")}</p>{conflicts.map(conflict=><article className="space-y-2 rounded-lg border p-4" key={conflict.id}><p className="break-all text-sm">{conflict.label} · {conflict.store}</p>{conflict.variants.map(variant=><div className="flex items-center justify-between gap-3" key={variant.id}><div className="min-w-0 break-words text-sm"><span>{variant.deleted?ui("删除版本"):variant.protected?(conflict.entity==="protected-domain"?`${ui("受保护页面树")} · ${variant.label}`:ui("受保护密文版本")):variant.label}</span>{variant.preview && <p className="mt-1 text-xs text-neutral-500">{variant.preview}</p>}</div><div className="flex shrink-0 flex-col gap-2 sm:flex-row"><button className="rounded border px-3 py-2" disabled={busy || active} onClick={()=>void resolve(conflict,variant.id)}>{ui("保留此版本")}</button><button className="rounded border px-3 py-2" disabled={busy || active || variant.deleted} onClick={()=>{setCopyChoice({conflict,variant:variant.id});setCopyPassword("");}}>{ui("保留两个版本")}</button></div></div>)}</article>)}</section>}
  </div></main>;
}
function SummaryView({title,summary}:{title:string;summary:Summary|null}){return <p className="text-sm">{title} · {ui("{p0} 项记录，{p1} 项删除，{p2} 个附件",{p0:String(summary?.records ?? 0),p1:String(summary?.deletions ?? 0),p2:String(summary?.blobs ?? 0)})}</p>;}
type Conflict={id:string;store:string;entity:string;key:string;label:string;variants:{id:string;label:string;deleted:boolean;protected:boolean;preview:string|null}[]};
