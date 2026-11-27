"""Fail CI when mandatory category validation or reviewed relational write ownership disappears.

Lexical source guard, not a Rust call-graph/type checker. Enforce the
unconditional first check at the authoritative writer and editor, reviewed
delegations, and reject new references to the reviewed low-level writers.
Existing bootstrap/migration call sites are count-limited; their internal
validation semantics are a separate audit.
"""
from collections import Counter
from pathlib import Path
import re
import runpy
import subprocess
import sys

HERE = Path(__file__).resolve().parent
REPO = HERE.parent
RUST = runpy.run_path(str(HERE / "check-egui-layout-placement.py"))
production = RUST["production"]
function_regions = RUST["function_regions"]
STORAGE = "crates/bando-data-access/src/storage.rs"
RELATIONAL = "crates/bando-data-access/src/relational_write.rs"
PRODUCT = "crates/bando-data-access/src/product_commands.rs"
CATALOG = "src-tauri/src/business_core/catalog_clients.rs"
CATEGORIES = "src-tauri/src/business_core/catalog_clients/categories.rs"
TAURI = "src-tauri/src/lib.rs"
# Match canonical exact call and propagated error, not comments or strings.
FIRST = {
    (STORAGE, "write_validated"): "crate::validate_category_transition(current,&state)?;",
    (CATALOG, "validate_product"): "bando_data_access::validate_product_assignment(state,product)?;",
}
REQUIRED = {
    (STORAGE, "write_v2_snapshot"): ["write_validated(&tx,&current,state,expected_revision,validate_orders,false)?"],
    (STORAGE, "finish_validated_update"): ["write_validated(tx,&current,state,expected_revision,validate_orders,prefer_workflow_patch)?"],
    (STORAGE, "write_validated"): ["crate::relational_write::store_v2_relational_delta(tx,current,&state)?"],
    (CATALOG, "apply_product_workbook"): ["bando_data_access::validate_category_transition(state,&next)?;"],
    (CATEGORIES, "validate"): ["bando_data_access::validate_category_transition(before,next)?;",
        "bando_data_access::validate_category_transition(&json!({:[],:[]}),next)?;"],
    (CATEGORIES, "legacy_add"): ["validate(Some(input),&next)?;"],
    (TAURI, "write_snapshot"): ["bando_data_access::write_v2_snapshot(conn,state,expected_revision,validate_orders)",
        "bando_data_access::validate_category_transition(&current,&state)?;"],
    (PRODUCT, "patch_product_v2"): ["crate::storage::update_v2_reviewed_workbook(conn,mutate)",
        "crate::storage::update_v2_category_change(conn,mutate)",
        "crate::storage::update_v2_entity_if_changed(conn,true,mutate)"],
    (RELATIONAL, "store_v2_relational_delta"): ["store_relational(tx,state,Some(before),None)"],
    (RELATIONAL, "store_v2_relational"): ["store_relational(tx,state,None,None)"],
    (RELATIONAL, "store_single_order_delta"): ["store_relational(tx,state,Some(before),Some(position))"],
}
# Include the bootstrap full-sync API and its private shared implementation.
# Existing migration entry points are allowed, not their use by new screens.
NARROW = re.compile(r"\b(?:store_v2_relational_delta|store_single_order_delta|store_v2_relational|store_relational)\b|\brelational_store\s*::\s*store\b")
ALLOWED = {
    (RELATIONAL,"store_v2_relational_delta","store_v2_relational_delta"): 1,
    (RELATIONAL,"store_single_order_delta","store_single_order_delta"): 1,
    (STORAGE,"write_validated","store_v2_relational_delta"): 1,
    (STORAGE,"finish_stable_freight","store_single_order_delta"): 1,
    (RELATIONAL,"store_v2_relational","store_v2_relational"): 1,
    (RELATIONAL,"store_v2_relational","store_relational"): 1,
    (RELATIONAL,"store_v2_relational_delta","store_relational"): 1,
    (RELATIONAL,"store_single_order_delta","store_relational"): 1,
    (RELATIONAL,"store_relational","store_relational"): 1,
    ("crates/bando-data-access/src/lib.rs",None,"store_v2_relational"): 1,
    ("crates/bando-data-access/src/recovery.rs","migrate","store_v2_relational"): 1,
    ("src-tauri/src/relational_store.rs","store","store_v2_relational"): 1,
    ("egui-app/src/test_workspace_init.rs","initialize_new","store_v2_relational"): 1,
    (TAURI,"write_snapshot","relational_store::store"): 1,
}

def compact(source):
    return re.sub(r"\s+", "", source)

def body_for(code, name):
    regions = list(function_regions(code,{name}))
    if len(regions) != 1:
        return None
    _, _, start, end = regions[0]
    return compact(code[start:end])

def check_contract(path, source):
    code = production(source)
    findings = []
    for (owner, fn), required in FIRST.items():
        if owner != path:
            continue
        body = body_for(code,fn)
        if body is None or not body.startswith(required):
            findings.append(f"{fn}: must begin with {required}")
    for (owner, fn), calls in REQUIRED.items():
        if owner != path:
            continue
        body = body_for(code,fn)
        for call in calls:
            if body is None or body.count(call)!=1:
                findings.append(f"{fn}: missing or repeated required delegation {call}")
    if path=="egui-app/src/test_workspace_init.rs":
        # Only the isolated, exclusive-create bootstrap is a reviewed full
        # writer. New product assignments must pass the authoritative validator.
        body=body_for(code,"initialize_new") or ""
        check="bando_data_access::validate_category_transition(&serde_json::json!({:[],:[]}),&state)?;"
        write="bando_data_access::store_v2_relational(&tx,&state)?;"
        if check+write not in body:
            findings.append("initialize_new: full seed write requires immediately preceding category validation")
    if path==RELATIONAL:
        # Verify the equality of the category collection as well as the
        # product collection in the order-only boundary. Match in the
        # original function source using offsets from the masked source.
        regions=list(function_regions(code,{"store_single_order_delta"}))
        if len(regions)!=1:
            findings.append("store_single_order_delta: missing scope guard")
        else:
            _,_,start,end=regions[0]
            body=compact(source[start:end])
            for field in ("products","categories"):
                if f'before["{field}"]!=state["{field}"]' not in body:
                    findings.append(f"store_single_order_delta: missing {field} equality guard")
    return findings

def ownership(path, source):
    code=production(source)
    regions=list(function_regions(code,{fn for owner,fn,_ in ALLOWED if owner==path and fn is not None}))
    counts=Counter()
    failures=[]
    for match in NARROW.finditer(code):
        fn=next((name for name,start,_,end in regions if start<=match.start()<end),None)
        token=compact(match.group())
        key=(path,fn,token)
        if key==("crates/bando-data-access/src/lib.rs",None,"store_v2_relational"):
            # Permit only the existing public re-export, never a module-level alias.
            line=code[code.rfind("\n",0,match.start())+1:code.find("\n",match.end()) if code.find("\n",match.end())!=-1 else len(code)]
            if compact(line)!="pubuserelational_write::store_v2_relational;":
                failures.append(f"{code.count(chr(10),0,match.start())+1}: unauthorized full-sync re-export")
                continue
        counts[key]+=1
        if counts[key]>ALLOWED.get(key,0):
            line=code.count("\n",0,match.start())+1
            failures.append(f"{line}: {match.group()} outside reviewed storage owner")
    return failures

def self_test():
    good="fn write_validated(){crate::validate_category_transition(current,&state)?;}"
    assert not [x for x in check_contract(STORAGE,good) if "must begin" in x]
    for bad in [
        'fn write_validated(){}',
        'fn write_validated(){/* crate::validate_category_transition(current,&state)?; */}',
        'fn write_validated(){let text="crate::validate_category_transition(current,&state)?;";}',
        'fn write_validated(){if false {crate::validate_category_transition(current,&state)?;}}',
        'fn write_validated(){let _=crate::validate_category_transition(current,&state);}',
        'fn write_validated(){crate::validate_category_transition(&state,current)?;}',
    ]:
        assert any("must begin" in x for x in check_contract(STORAGE,bad)),bad
    assert ownership("egui-app/src/new.rs","fn write(){store_v2_relational_delta(tx,before,next);}")
    assert ownership("egui-app/src/new.rs","use crate::storage::store_single_order_delta as raw;")
    assert not ownership("new.rs","#[cfg(test)] mod tests {fn t(){store_single_order_delta();}}")
    assert ownership("new.rs","#[cfg(test)] mod tests {} fn write(){store_single_order_delta();}")
    assert not ownership(STORAGE,"fn write_validated(){store_v2_relational_delta();}")
    assert ownership(STORAGE,"fn write_validated(){store_v2_relational_delta();store_v2_relational_delta();}")
    assert ownership(STORAGE,"fn new_write(){store_v2_relational_delta();}")
    assert ownership("egui-app/src/new.rs","fn write(){bando_data_access::store_v2_relational(tx,state);}")
    assert ownership("egui-app/src/new.rs","use bando_data_access::store_v2_relational as raw;")
    assert ownership("src-tauri/src/new.rs","fn write(){relational_store :: store(tx,state);}")
    assert not ownership("crates/bando-data-access/src/recovery.rs","fn migrate(){crate::store_v2_relational(tx,state)?;}")
    assert not ownership("crates/bando-data-access/src/lib.rs","pub use relational_write::store_v2_relational;")
    bootstrap="egui-app/src/test_workspace_init.rs"
    assert not ownership(bootstrap,"fn initialize_new(){bando_data_access::store_v2_relational(&tx,&state)?;}")
    assert check_contract(bootstrap,"fn initialize_new(){bando_data_access::store_v2_relational(&tx,&state)?;}")
    assert ownership(bootstrap,"fn other(){bando_data_access::store_v2_relational(&tx,&state)?;}")

def main():
    self_test()
    paths=subprocess.check_output(["git","ls-files","-z","--","*.rs"],cwd=REPO).decode("utf-8").split("\0")
    required={p for p,_ in FIRST}|{p for p,_ in REQUIRED}|{RELATIONAL}
    checked=set()
    failures=[]
    count=0
    for path in sorted(p for p in paths if p):
        source=(REPO/path).read_text(encoding="utf-8-sig")
        # The existing source guard masks inline test modules. Out-of-line
        # fixture files are omitted; they are not runtime save entry points.
        if re.match(r"\s*#!\s*\[\s*cfg\s*\(\s*test\s*\)\s*\]",RUST["strip_literals"](source)):
            continue
        count+=1
        checked.add(path)
        failures.extend(f"{path}: {msg}" for msg in check_contract(path,source))
        failures.extend(f"{path}:{msg}" for msg in ownership(path,source))
    failures.extend(f"{path}: missing required owner" for path in sorted(required-checked))
    for error in failures:
        print(error,file=sys.stderr)
    print(f"category assignment save contract: {count} Rust files, {len(failures)} violation(s)")
    return int(bool(failures))

if __name__=="__main__":
    sys.exit(main())
