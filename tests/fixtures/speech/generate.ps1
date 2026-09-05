$ErrorActionPreference = 'Stop'
$voice = New-Object -ComObject SAPI.SpVoice
$voices = $voice.GetVoices()
if ($voices.Count -eq 0) { throw 'No local SAPI voice is installed.' }
for ($i = 0; $i -lt $voices.Count; $i++) {
    if ($voices.Item($i).GetDescription() -match 'Maria|Portuguese|Brasil') {
        $voice.Voice = $voices.Item($i)
        break
    }
}
foreach ($sample in @(@{Name='oi';Text='oi'}, @{Name='sim';Text='sim'}, @{Name='frase-curta';Text='Bom dia, tudo bem?'})) {
    $stream = New-Object -ComObject SAPI.SpFileStream
    $stream.Format.Type = 18 # PCM mono 16-bit, 16 kHz
    $stream.Open((Join-Path $PSScriptRoot ($sample.Name + '.wav')), 3, $false)
    try {
        $voice.AudioOutputStream = $stream
        [void]$voice.Speak($sample.Text)
    } finally { $stream.Close() }
}
Write-Output 'Generated three synthetic speech fixtures locally with Windows SAPI.'
