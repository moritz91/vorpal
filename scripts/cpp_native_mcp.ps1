param(
  [Parameter(Mandatory = $true)][string]$VorpalPath,
  [Parameter(Mandatory = $true)][string]$SourcePath,
  [Parameter(Mandatory = $true)][string]$IndexPath,
  [Parameter(Mandatory = $true)][string]$ConfigPath,
  [Parameter(Mandatory = $true)][string]$VsDevShellPath,
  [switch]$NoWatchRebuild,
  [switch]$NoAutoWarm
)

$ErrorActionPreference = 'Stop'
foreach ($inputPath in @($VorpalPath, $ConfigPath, $VsDevShellPath)) {
  if (-not (Test-Path -LiteralPath $inputPath -PathType Leaf)) {
    throw 'A required native MCP input file is unavailable'
  }
}
if (-not (Test-Path -LiteralPath $SourcePath -PathType Container)) {
  throw 'The authorized source directory is unavailable'
}
$VorpalPath = (Resolve-Path -LiteralPath $VorpalPath).ProviderPath
$ConfigPath = (Resolve-Path -LiteralPath $ConfigPath).ProviderPath
$VsDevShellPath = (Resolve-Path -LiteralPath $VsDevShellPath).ProviderPath
$SourcePath = (Resolve-Path -LiteralPath $SourcePath).ProviderPath
$IndexPath = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($IndexPath)
$sourceDirectory = $SourcePath.TrimEnd('\', '/')
$indexDirectory = $IndexPath
if ($indexDirectory.Equals($sourceDirectory, [StringComparison]::OrdinalIgnoreCase) -or
    $indexDirectory.StartsWith($sourceDirectory + '\', [StringComparison]::OrdinalIgnoreCase)) {
  throw 'Use an external index directory for the native MCP source checkout'
}
[Console]::InputEncoding = New-Object System.Text.UTF8Encoding($false)
[Console]::OutputEncoding = New-Object System.Text.UTF8Encoding($false)
& $VsDevShellPath -Arch amd64 -HostArch amd64 -SkipAutomaticLocation | Out-Null
if ($NoAutoWarm) { $env:VORPAL_NO_AUTOWARM = '1' }
$mcpArguments = @('mcp', '--src', $SourcePath, '--index', $IndexPath,
  '--config', $ConfigPath, '--profile', 'full')
if ($NoWatchRebuild) { $mcpArguments += '--no-watch-rebuild' }
& $VorpalPath @mcpArguments
exit $LASTEXITCODE
