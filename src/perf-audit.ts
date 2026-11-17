/** Synthetic Windows CI only: counts direct React function render invocations,
 * including development StrictMode repeats; not DOM mutations/commits.
 * The flag is statically disabled by Vite in production. */
export function auditReactRender(component: string): void {
  if (import.meta.env.VITE_BANDO_NATIVE_AUDIT !== "true" || typeof window === "undefined") return;
  const trace = (window as Window & { __bandoRenderTrace?: string[] }).__bandoRenderTrace;
  if (Array.isArray(trace)) trace.push(component);
}

export type AuditEvent={at:number;source:string;phase:string;info?:Record<string,string|number|boolean>};
/** CI-only native -> state -> React chronology without private order/client data. */
export function auditStateTransition(
  source:string,phase:string,info?:Record<string,string|number|boolean>,
):void{
  if(import.meta.env.VITE_BANDO_NATIVE_AUDIT!=="true"||typeof window==="undefined")return;
  const trace=(window as Window & {__bandoStateTrace?:AuditEvent[]}).__bandoStateTrace;
  if(Array.isArray(trace))trace.push({at:performance.now(),source,phase,info});
}
