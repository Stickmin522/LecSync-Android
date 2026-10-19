# Build

[English](../README.md) · [简体中文](../README.zh-CN.md) · [日本語](../README.ja.md)

## Requirements

Use Windows with PowerShell 7 and an ASCII checkout path such as `C:\src\LingoDesk`.

| Tool | Version |
|---|---|
| Flutter / Dart | 3.47.6 / 3.13.5 |
| Rust | 1.99.0 |
| Android SDK | android-37 |
| Android Build Tools | 37.0.0 |
| Android NDK | 26.1.10909125 |
| Java | JDK 17 or newer |
| Gradle / AGP | Wrapper 9.5.0 / 9.1.0 |

Add Flutter and Rust to PATH, install the Android components above, and set the tool paths for your machine:

```powershell
$env:ANDROID_HOME = 'C:\path\to\Android\Sdk'
$env:JAVA_HOME = 'C:\path\to\jdk'
$env:FLUTTER_ROOT = 'C:\path\to\flutter'
rustup target add aarch64-linux-android
.\build.ps1
```

`FLUTTER_ROOT` is optional when Flutter is on PATH. `ANDROID_SDK_ROOT` is accepted when `ANDROID_HOME` is unset. The output is `outputs/lingodesk-1.2.3-arm64.apk`.

The first build creates a signing key in `.signing/`. Keep it private and use it for subsequent builds. Your signature differs from the published APK, so your build cannot update that installation. Change `applicationId` in `app/android/app/build.gradle.kts` if you want a separate installation.

Set `LINGODESK_SIGNING_DIR` to reuse a private signing directory stored outside the checkout. It must contain `release.jks` and `key.properties`.

## Checks

```powershell
Push-Location app
flutter pub get
flutter analyze
flutter test
Pop-Location
cargo test --manifest-path rust/Cargo.toml --locked
```

To check your APK, obtain its signing certificate fingerprint and pass it to the release checker:

```powershell
& "$env:ANDROID_HOME\build-tools\37.0.0\apksigner.bat" verify --print-certs 'outputs\lingodesk-1.2.3-arm64.apk'
python tests/verify_release.py --certificate-sha256 YOUR_PUBLIC_CERTIFICATE_SHA256
```

Use `--application-id YOUR_APPLICATION_ID` if you changed the application ID.

## 简体中文

准备上表工具链，设置自己的工具路径，再运行 `.\build.ps1`。首次构建会创建 `.signing/`，请保留其中的签名用于后续更新，并勿上传。自行编译的签名与发布版不同，不能直接覆盖发布版安装。

## 日本語

表のツールを準備し、自分の環境に合わせてパスを設定してから `.\build.ps1` を実行します。初回に作成される `.signing/` は、更新に使うため保管し、公開しないでください。自分でビルドした APK は公開版と署名が異なり、そのインストールを直接更新できません。
