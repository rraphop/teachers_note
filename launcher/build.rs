// 실행기에 넣을 프로그램 묶음(TN_PAYLOAD)과 파일 목록(TN_MANIFEST)은 패키징 스크립트가 만든다.
// 아이콘·파일 설명·관리자 권한 없이 실행(asInvoker) 정보를 EXE 에 넣는다.

const MANIFEST: &str = r#"<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
    <security><requestedPrivileges><requestedExecutionLevel level="asInvoker" uiAccess="false"/></requestedPrivileges></security>
  </trustInfo>
  <application xmlns="urn:schemas-microsoft-com:asm.v3">
    <windowsSettings><dpiAware xmlns="http://schemas.microsoft.com/SMI/2005/WindowsSettings">true</dpiAware></windowsSettings>
  </application>
</assembly>"#;

fn main() {
    for key in ["TN_PAYLOAD", "TN_MANIFEST"] {
        println!("cargo:rerun-if-env-changed={key}");
        match std::env::var(key) {
            Ok(p) => println!("cargo:rerun-if-changed={p}"),
            Err(_) => panic!("{key} 가 없습니다. scripts\\package-portable.ps1 로 빌드해 주세요."),
        }
    }
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = tauri_winres::WindowsResource::new();
        res.set_icon("../src-tauri/icons/icon.ico");
        res.set("FileDescription", "로컬 AI 교무수첩");
        res.set("ProductName", "로컬 AI 교무수첩");
        res.set("OriginalFilename", "TeacherNotebook.exe");
        res.set("LegalCopyright", "Copyright (c) 2026 차일영. All rights reserved.");
        res.set("CompanyName", "차일영");
        res.set_manifest(MANIFEST);
        res.compile().expect("아이콘·버전 정보를 넣지 못했습니다");
    }
}
