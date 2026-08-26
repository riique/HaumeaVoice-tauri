param(
    [Parameter(Mandatory = $true)]
    [ValidatePattern('^[a-p]{32}$')]
    [string]$ExtensionId,

    [Parameter(Mandatory = $true)]
    [ValidateScript({ Test-Path -LiteralPath $_ -PathType Leaf })]
    [string]$ExecutablePath
)

$ErrorActionPreference = 'Stop'
$resolvedExecutable = (Resolve-Path -LiteralPath $ExecutablePath).Path
$targetDirectory = Join-Path $env:LOCALAPPDATA 'HaumeaVoice\NativeMessaging'
$targetManifest = Join-Path $targetDirectory 'com.haumeavoice.context.json'
$template = Join-Path $PSScriptRoot 'com.haumeavoice.context.json.template'

New-Item -ItemType Directory -Path $targetDirectory -Force | Out-Null
$content = Get-Content -LiteralPath $template -Raw
$content = $content.Replace('__EXECUTABLE_PATH__', $resolvedExecutable.Replace('\', '\\'))
$content = $content.Replace('__EXTENSION_ID__', $ExtensionId)
Set-Content -LiteralPath $targetManifest -Value $content -Encoding UTF8

$registryPaths = @(
    'HKCU:\Software\Google\Chrome\NativeMessagingHosts\com.haumeavoice.context',
    'HKCU:\Software\Microsoft\Edge\NativeMessagingHosts\com.haumeavoice.context',
    'HKCU:\Software\BraveSoftware\Brave-Browser\NativeMessagingHosts\com.haumeavoice.context'
)
foreach ($registryPath in $registryPaths) {
    New-Item -Path $registryPath -Force | Out-Null
    Set-ItemProperty -Path $registryPath -Name '(default)' -Value $targetManifest
}

Write-Host "Native Messaging host registrado em $targetManifest"
