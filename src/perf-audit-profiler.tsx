import { Profiler, type ProfilerOnRenderCallback, type ReactNode } from "react";
/** Audit-only React commit timings. Nested durations overlap, do not add them. */
type Commit = {id:string;phase:string;actualDuration:number;baseDuration:number;startTime:number;commitTime:number};
const onRender:ProfilerOnRenderCallback=(id,phase,actualDuration,baseDuration,startTime,commitTime)=>{
  if(typeof window==="undefined")return;
  const trace=(window as Window & {__bandoProfilerTrace?:Commit[]}).__bandoProfilerTrace;
  if(Array.isArray(trace))trace.push({id,phase,actualDuration,baseDuration,startTime,commitTime});
};
export function AuditProfiler({id,children}:{id:string;children:ReactNode}){
  if(import.meta.env.VITE_BANDO_NATIVE_AUDIT!=="true")return <>{children}</>;
  return <Profiler id={id} onRender={onRender}>{children}</Profiler>;
}
