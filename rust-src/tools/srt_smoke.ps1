# Runs a packaged srtstripper over one fixture subtitle and asserts the
# tool's own report: the binary starts, prints a PASS line, and writes the
# stripped text next to the subtitle. Every bundle job runs this, so a
# pinned build that cannot start on its platform fails the job.
param([Parameter(Mandatory = $true)][string]$Exe)
$ErrorActionPreference = "Stop"
$bin = (Resolve-Path $Exe).Path
$work = Join-Path ([System.IO.Path]::GetTempPath()) ("srt-smoke-" + [guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Force $work | Out-Null
try {
    $body = "1`r`n00:00:01,000 --> 00:00:03,000`r`nHello world.`r`n`r`n2`r`n00:00:04,000 --> 00:00:06,000`r`nSecond line.`r`n"
    [System.IO.File]::WriteAllText((Join-Path $work "sample.en.srt"), $body)
    $out = Join-Path $work "stdout.txt"
    $err = Join-Path $work "stderr.txt"
    # the windows build is GUI-subsystem, so the call operator does not
    # wait and leaves no exit code; Start-Process waits on the handle
    $p = Start-Process -FilePath $bin -WorkingDirectory $work -ArgumentList "sample.en.srt" -Wait -PassThru -RedirectStandardOutput $out -RedirectStandardError $err
    $text = Get-Content $out -Raw
    if ($p.ExitCode -ne 0) { throw "srtstripper rc $($p.ExitCode): $text $(Get-Content $err -Raw)" }
    if ($text -notmatch "(?m)^PASS:") { throw "srtstripper printed no PASS line: $text" }
    $m = [regex]::Matches($text, " -> (.+)$")
    if ($m.Count -eq 0) { throw "srtstripper PASS line carries no output path: $text" }
    $txt = Join-Path $work $m[$m.Count - 1].Groups[1].Value.Trim()
    if (-not (Test-Path $txt)) { throw "srtstripper output missing: $txt" }
    $lines = Get-Content $txt
    if (($lines -notcontains "Hello world.") -or ($lines -notcontains "Second line.")) {
        throw "stripped text lost the dialogue: $($lines -join '|')"
    }
    if ($lines -match "-->") { throw "timecode survived the strip" }
    Write-Host "srtstripper smoke passed: $(Split-Path $txt -Leaf)"
}
finally {
    Remove-Item -Recurse -Force $work -ErrorAction SilentlyContinue
}
