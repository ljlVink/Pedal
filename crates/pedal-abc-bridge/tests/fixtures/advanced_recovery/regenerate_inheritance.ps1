# 重建继承回归字节码；普通 cargo test 使用已提交的 ABC，无需 SDK。
# @author sky
param([string] $SdkRoot = $env:OHOS_HOME)
$ErrorActionPreference = 'Stop'
if (-not $SdkRoot) { $SdkRoot = 'C:/harmony_sdk/default/openharmony' }
$compiler = Join-Path $SdkRoot 'ets/build-tools/ets-loader/bin/ark/build-win/bin/es2abc.exe'
if (-not (Test-Path -LiteralPath $compiler)) { throw "Missing compiler: $compiler" }
Push-Location $PSScriptRoot
try {
    $common = @('--module', '--merge-abc', '--target-api-version=12', '--record-name=ImportedInheritance', '--source-file=ImportedInheritance.ts')
    & $compiler @common --debug-info --output=inheritance-debug.abc ImportedInheritance.ts
    if ($LASTEXITCODE -ne 0) { throw "Inheritance debug fixture compilation failed: $LASTEXITCODE" }
    & $compiler @common --opt-level=2 --output=inheritance-release.abc ImportedInheritance.ts
    if ($LASTEXITCODE -ne 0) { throw "Inheritance release fixture compilation failed: $LASTEXITCODE" }
} finally {
    Pop-Location
}
