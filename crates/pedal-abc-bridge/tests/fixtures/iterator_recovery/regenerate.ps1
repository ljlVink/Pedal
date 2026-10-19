# 重新生成迭代器与异常边界回归字节码；正常测试无需 SDK。
# @author sky
param([string] $SdkRoot = $env:OHOS_HOME)
$ErrorActionPreference = 'Stop'
if (-not $SdkRoot) { $SdkRoot = 'C:\harmony_sdk\default\openharmony' }
$compiler = Join-Path $SdkRoot 'ets/build-tools/ets-loader/bin/ark/build-win/bin/es2abc.exe'
Push-Location $PSScriptRoot
try {
    $common = @('--module', '--merge-abc', '--target-api-version=12', '--extension=ts', '--record-name=IteratorRecovery', '--source-file=IteratorRecovery.ts')
    & $compiler @common --debug-info --output=debug.abc IteratorRecovery.ts
    if ($LASTEXITCODE -ne 0) { throw 'Debug fixture compilation failed' }
    & $compiler @common --opt-level=2 --output=release.abc IteratorRecovery.ts
    if ($LASTEXITCODE -ne 0) { throw 'Release fixture compilation failed' }
} finally { Pop-Location }
