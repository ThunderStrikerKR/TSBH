//! Local account isolation for private preview data and OCR working directories.
//! This is filesystem access control, not encryption or a boundary against
//! administrators or processes already running as the same user.
use std::{fs,path::{Path,PathBuf}};

fn check_entry(path:&Path,directory:bool)->Result<(),String>{
    let meta=fs::symlink_metadata(path).map_err(|_|"개인정보 저장 경로 확인 실패")?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if meta.file_attributes()&0x400!=0{return Err("개인정보 저장 경로의 연결 지점은 허용되지 않습니다.".into())}
    }
    if meta.file_type().is_symlink()||(directory&&!meta.is_dir())||(!directory&&!meta.is_file()){
        return Err("개인정보 저장 경로의 파일 형식 오류".into());
    }
    Ok(())
}

#[cfg(windows)]
fn restrict(path:&Path,directory:bool)->Result<(),String>{
    use std::{os::windows::ffi::OsStrExt,ptr};
    use windows_sys::Win32::{
        Foundation::{CloseHandle,LocalFree,HANDLE},
        Security::{GetTokenInformation,TokenUser,TOKEN_USER,TOKEN_QUERY,SetFileSecurityW,
            DACL_SECURITY_INFORMATION,PROTECTED_DACL_SECURITY_INFORMATION},
        Security::Authorization::{ConvertSidToStringSidW,ConvertStringSecurityDescriptorToSecurityDescriptorW},
        System::Threading::{GetCurrentProcess,OpenProcessToken},
    };
    // Aligned storage for TOKEN_USER and its inline SID.
    struct Handle(HANDLE);
    impl Drop for Handle{fn drop(&mut self){unsafe{CloseHandle(self.0);}}}
    struct Local(*mut std::ffi::c_void);
    impl Drop for Local{fn drop(&mut self){unsafe{LocalFree(self.0);}}}
    let error=||"Windows 개인정보 파일 접근 권한을 설정하지 못했습니다.".to_owned();
    unsafe{
        let mut token=ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(),TOKEN_QUERY,&mut token)==0{return Err(error())}
        let token=Handle(token);
        let mut needed=0u32;
        GetTokenInformation(token.0,TokenUser,ptr::null_mut(),0,&mut needed);
        if needed==0||needed>65536{return Err(error())}
        let mut storage=vec![0usize;(needed as usize).div_ceil(std::mem::size_of::<usize>())];
        if GetTokenInformation(token.0,TokenUser,storage.as_mut_ptr().cast(),needed,&mut needed)==0{return Err(error())}
        let user=&*storage.as_ptr().cast::<TOKEN_USER>();
        let mut sid=ptr::null_mut();
        if ConvertSidToStringSidW(user.User.Sid,&mut sid)==0{return Err(error())}
        let sid_guard=Local(sid.cast());
        let mut len=0;
        while len<256&&*sid.add(len)!=0{len+=1}
        if len==256{return Err(error())}
        let sid_string=String::from_utf16(std::slice::from_raw_parts(sid,len)).map_err(|_|error())?;
        drop(sid_guard);
        let inheritance=if directory{"OICI"}else{""};
        // Protected DACL replaces existing discretionary grants, including
        // inherited broad access. SYSTEM and the current token user only.
        let sddl=format!("D:P(A;{inheritance};FA;;;SY)(A;{inheritance};FA;;;{sid_string})");
        let wide:Vec<u16>=sddl.encode_utf16().chain(Some(0)).collect();
        let mut sd=ptr::null_mut();
        if ConvertStringSecurityDescriptorToSecurityDescriptorW(wide.as_ptr(),1,&mut sd,ptr::null_mut())==0{return Err(error())}
        let sd=Local(sd);
        let name:Vec<u16>=path.as_os_str().encode_wide().chain(Some(0)).collect();
        if SetFileSecurityW(name.as_ptr(),DACL_SECURITY_INFORMATION|PROTECTED_DACL_SECURITY_INFORMATION,sd.0)==0{
            return Err(error());
        }
    }
    Ok(())
}
#[cfg(unix)]
fn restrict(path:&Path,directory:bool)->Result<(),String>{
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path,fs::Permissions::from_mode(if directory{0o700}else{0o600}))
        .map_err(|_|"개인정보 파일 접근 권한 설정 실패".into())
}
#[cfg(not(any(windows,unix)))]
fn restrict(_path:&Path,_directory:bool)->Result<(),String>{
    Err("개인정보 파일 접근 권한을 지원하지 않는 환경입니다.".into())
}

pub(crate) fn private_directory(path:&Path)->Result<PathBuf,String>{
    if !path.is_absolute(){return Err("개인정보 저장 폴더는 절대 경로여야 합니다.".into())}
    let parent=path.parent().ok_or("개인정보 저장 상위 경로 오류")?;
    // Existing OS user profile root only. Do not change its permissions.
    let canonical_parent=parent.canonicalize().map_err(|_|"개인정보 저장 상위 경로 확인 실패")?;
    let expected=canonical_parent.join(path.file_name().ok_or("개인정보 저장 폴더 이름 오류")?);
    match fs::create_dir(&expected){
        Ok(())=>{},
        Err(e) if e.kind()==std::io::ErrorKind::AlreadyExists=>{},
        Err(_)=>return Err("개인정보 저장 폴더 생성 실패".into()),
    }
    check_entry(&expected,true)?;
    if expected.canonicalize().map_err(|_|"개인정보 저장 폴더 확인 실패")?!=expected{
        return Err("개인정보 저장 폴더가 변경되었습니다.".into())
    }
    restrict(&expected,true)?;
    Ok(expected)
}
fn protect_file(path:&Path)->Result<(),String>{
    check_entry(path,false)?;
    restrict(path,false)
}
pub(crate) fn protect_sqlite_files(path:&Path)->Result<(),String>{
    protect_file(path)?;
    for suffix in ["-wal","-shm","-journal"]{
        let mut name=path.as_os_str().to_os_string();name.push(suffix);
        let side=PathBuf::from(name);
        match fs::symlink_metadata(&side){
            Ok(_)=>protect_file(&side)?,
            Err(e) if e.kind()==std::io::ErrorKind::NotFound=>{},
            Err(_)=>return Err("개인정보 DB 보조 파일 확인 실패".into()),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests{
    use super::*;
    struct Fixture(PathBuf);
    impl Fixture{
        fn new()->Self{
            let p=std::env::temp_dir().join(format!("bando-private-fixture-{}",uuid::Uuid::new_v4()));
            fs::create_dir(&p).unwrap();Self(p)
        }
    }
    impl Drop for Fixture{fn drop(&mut self){let _=fs::remove_dir_all(&self.0);}}
    #[cfg(windows)]
    #[test]
    fn windows_dacl_is_protected_with_only_two_access_entries(){
        use std::{os::windows::ffi::OsStrExt,ptr};
        use windows_sys::Win32::Security::{GetFileSecurityW,GetSecurityDescriptorControl,
            GetSecurityDescriptorDacl,DACL_SECURITY_INFORMATION,ACL,SE_DACL_PROTECTED};
        let root=Fixture::new();let dir=private_directory(&root.0.join("private")).unwrap();
        let db=dir.join("synthetic.sqlite");fs::write(&db,b"synthetic").unwrap();
        protect_sqlite_files(&db).unwrap();
        for path in [&dir,&db]{
            let name:Vec<u16>=path.as_os_str().encode_wide().chain(Some(0)).collect();
            unsafe{
                let mut needed=0;
                GetFileSecurityW(name.as_ptr(),DACL_SECURITY_INFORMATION,ptr::null_mut(),0,&mut needed);
                assert!(needed>0&&needed<65536);
                let mut buffer=vec![0usize;(needed as usize).div_ceil(std::mem::size_of::<usize>())];
                assert_ne!(GetFileSecurityW(name.as_ptr(),DACL_SECURITY_INFORMATION,buffer.as_mut_ptr().cast(),needed,&mut needed),0);
                let mut control=0;let mut revision=0;
                assert_ne!(GetSecurityDescriptorControl(buffer.as_mut_ptr().cast(),&mut control,&mut revision),0);
                assert_ne!(control&SE_DACL_PROTECTED,0);
                let mut present=0;let mut defaulted=0;let mut acl:*mut ACL=ptr::null_mut();
                assert_ne!(GetSecurityDescriptorDacl(buffer.as_mut_ptr().cast(),&mut present,&mut acl,&mut defaulted),0);
                assert_ne!(present,0);assert!(!acl.is_null());assert_eq!((*acl).AceCount,2);
            }
        }
    }
    #[test]
    fn protects_existing_bytes_without_rewriting_or_deleting(){
        let root=Fixture::new();
        let dir=private_directory(&root.0.join("private")).unwrap();
        let db=dir.join("synthetic.sqlite");fs::write(&db,b"synthetic evidence").unwrap();
        let mut side=db.as_os_str().to_os_string();side.push("-wal");let side=PathBuf::from(side);
        fs::write(&side,b"synthetic wal").unwrap();
        protect_sqlite_files(&db).unwrap();
        assert_eq!(fs::read(&db).unwrap(),b"synthetic evidence");
        assert_eq!(fs::read(&side).unwrap(),b"synthetic wal");
        #[cfg(unix)]{
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(fs::metadata(&dir).unwrap().permissions().mode()&0o777,0o700);
            assert_eq!(fs::metadata(&db).unwrap().permissions().mode()&0o777,0o600);
            assert_eq!(fs::metadata(&side).unwrap().permissions().mode()&0o777,0o600);
        }
    }
    #[test]
    fn rejects_non_file_sidecar_and_relative_directory(){
        let root=Fixture::new();let dir=private_directory(&root.0.join("private")).unwrap();
        let db=dir.join("synthetic.sqlite");fs::write(&db,b"data").unwrap();
        let mut side=db.as_os_str().to_os_string();side.push("-wal");fs::create_dir(PathBuf::from(side)).unwrap();
        assert!(protect_sqlite_files(&db).is_err());
        assert!(private_directory(Path::new("relative")).is_err());
    }
    #[cfg(unix)]
    #[test]
    fn refuses_symbolic_link_without_touching_target(){
        use std::os::unix::fs::{symlink,PermissionsExt};
        let root=Fixture::new();let target=root.0.join("target");
        fs::create_dir(&target).unwrap();fs::set_permissions(&target,fs::Permissions::from_mode(0o755)).unwrap();
        let link=root.0.join("link");symlink(&target,&link).unwrap();
        assert!(private_directory(&link).is_err());
        assert_eq!(fs::metadata(&target).unwrap().permissions().mode()&0o777,0o755);
    }
}
