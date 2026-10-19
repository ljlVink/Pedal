# 生成独立类型转换回归；普通 cargo test 无需 SDK。
# @author sky
param([string] $SdkRoot = $env:OHOS_HOME)
$ErrorActionPreference = 'Stop'
if (-not $SdkRoot) { $SdkRoot = 'C:\harmony_sdk\default\openharmony' }
$compiler = Join-Path $SdkRoot 'ets/build-tools/ets-loader/bin/ark/build-win/bin/es2abc.exe'
Push-Location $PSScriptRoot
try {
    $common = @('--module', '--merge-abc', '--target-api-version=12', '--extension=ts', '--record-name=CoercionRecovery', '--source-file=CoercionRecovery.ts')
    & $compiler @common --debug-info --output=debug.abc CoercionRecovery.ts
    if ($LASTEXITCODE -ne 0) { throw "Debug compilation failed: $LASTEXITCODE" }
    & $compiler @common --opt-level=2 --output=release.abc CoercionRecovery.ts
    if ($LASTEXITCODE -ne 0) { throw "Release compilation failed: $LASTEXITCODE" }
} finally { Pop-Location }
