# 重新生成属性与异步 rest 参数回归字节码；普通测试无需 SDK。
# @author sky
param([string] $SdkRoot = $env:OHOS_HOME)
$ErrorActionPreference = 'Stop'
if (-not $SdkRoot) { $SdkRoot = 'C:\harmony_sdk\default\openharmony' }
$compiler = Join-Path $SdkRoot 'ets/build-tools/ets-loader/bin/ark/build-win/bin/es2abc.exe'
if (-not (Test-Path -LiteralPath $compiler)) { throw "Missing compiler: $compiler" }
Push-Location $PSScriptRoot
try {
    $common = @('--module', '--merge-abc', '--target-api-version=12', '--extension=ts', '--record-name=PropertyRecovery', '--source-file=PropertyRecovery.ts')
    & $compiler @common --debug-info --output=debug.abc PropertyRecovery.ts
    if ($LASTEXITCODE -ne 0) { throw "Debug fixture compilation failed: $LASTEXITCODE" }
    & $compiler @common --opt-level=2 --output=release.abc PropertyRecovery.ts
    if ($LASTEXITCODE -ne 0) { throw "Release fixture compilation failed: $LASTEXITCODE" }
} finally { Pop-Location }
