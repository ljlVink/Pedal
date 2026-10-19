# 重新生成独立回归字节码；普通 cargo test 无需安装 SDK。
# @author sky
param(
    [string] $SdkRoot = $env:OHOS_HOME
)
$ErrorActionPreference = 'Stop'
if (-not $SdkRoot) { $SdkRoot = 'C:\harmony_sdk\default\openharmony' }
$compiler = Join-Path $SdkRoot 'ets/build-tools/ets-loader/bin/ark/build-win/bin/es2abc.exe'
if (-not (Test-Path -LiteralPath $compiler)) { throw "Missing compiler: $compiler" }
Push-Location $PSScriptRoot
try {
    $common = @('--module', '--merge-abc', '--target-api-version=12', '--extension=ts', '--record-name=AdvancedRecovery', '--source-file=AdvancedRecovery.ets')
    & $compiler @common --debug-info --output=debug.abc AdvancedRecovery.ets
    if ($LASTEXITCODE -ne 0) { throw "Debug fixture compilation failed: $LASTEXITCODE" }
    & $compiler @common --opt-level=2 --output=release.abc AdvancedRecovery.ets
    if ($LASTEXITCODE -ne 0) { throw "Release fixture compilation failed: $LASTEXITCODE" }
    $arkui = @('--module', '--merge-abc', '--target-api-version=12', '--record-name=ArkUIProbe', '--source-file=ArkUIProbe.ts')
    & $compiler @arkui --debug-info --output=arkui-debug.abc ArkUIProbe.ts
    if ($LASTEXITCODE -ne 0) { throw "ArkUI debug fixture compilation failed: $LASTEXITCODE" }
    & $compiler @arkui --opt-level=2 --output=arkui-release.abc ArkUIProbe.ts
    if ($LASTEXITCODE -ne 0) { throw "ArkUI release fixture compilation failed: $LASTEXITCODE" }
    $properties = @('--module', '--merge-abc', '--target-api-version=12', '--record-name=PropertyDefinitions', '--source-file=PropertyDefinitions.ts')
    & $compiler @properties --debug-info --output=properties-debug.abc PropertyDefinitions.ts
    if ($LASTEXITCODE -ne 0) { throw "Property debug fixture compilation failed: $LASTEXITCODE" }
    & $compiler @properties --opt-level=2 --output=properties-release.abc PropertyDefinitions.ts
    if ($LASTEXITCODE -ne 0) { throw "Property release fixture compilation failed: $LASTEXITCODE" }
    $concurrent = @('--module', '--merge-abc', '--target-api-version=12', '--extension=ts', '--record-name=ConcurrentRecovery', '--source-file=ConcurrentRecovery.ts')
    & $compiler @concurrent --debug-info --output=concurrent-debug.abc ConcurrentRecovery.ts
    if ($LASTEXITCODE -ne 0) { throw "Concurrent debug fixture compilation failed: $LASTEXITCODE" }
    & $compiler @concurrent --opt-level=2 --output=concurrent-release.abc ConcurrentRecovery.ts
    if ($LASTEXITCODE -ne 0) { throw "Concurrent release fixture compilation failed: $LASTEXITCODE" }
} finally {
    Pop-Location
}
