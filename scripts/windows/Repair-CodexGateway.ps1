#requires -Version 5.1
<#
.SYNOPSIS
Diagnoses (and optionally repairs) the Windows Codex -> local Antigravity gateway path.

.DESCRIPTION
Read-only by default. It inspects %USERPROFILE%\.codex\config.toml, auth.json and the
native Antigravity Tools gui_config.json, probes the gateway without using the system
proxy, and reports key drift, proxy bypass and listener ownership problems.

With -Apply it backs up both Codex files, synchronises the bearer token and base URL
(optionally from the native gateway config), and persists NO_PROXY for the current
user. Secrets are never printed; only length and a short SHA-256 prefix are shown.

.PARAMETER CodexHome
Codex configuration directory. Defaults to %USERPROFILE%\.codex.

.PARAMETER GatewayBaseUrl
OpenAI-compatible gateway base URL. Defaults to http://127.0.0.1:8045/v1.

.PARAMETER NativeGatewayConfig
Native Antigravity Tools configuration. Defaults to
%USERPROFILE%\.antigravity_tools\gui_config.json.

.PARAMETER Apply
Write the repairs. Without it the script only reports.

.PARAMETER UseNativeGateway
Take the API key from the native gateway config instead of keeping the Codex token.

.PARAMETER SelfTest
Run offline unit checks of the parsing/update helpers and exit.

.EXAMPLE
powershell -NoProfile -ExecutionPolicy Bypass -File scripts\windows\Repair-CodexGateway.ps1

.EXAMPLE
powershell -NoProfile -ExecutionPolicy Bypass -File scripts\windows\Repair-CodexGateway.ps1 -Apply -UseNativeGateway
#>
[CmdletBinding()]
param(
    [string]$CodexHome = (Join-Path $env:USERPROFILE '.codex'),
    [string]$GatewayBaseUrl = 'http://127.0.0.1:8045/v1',
    [string]$NativeGatewayConfig = (Join-Path $env:USERPROFILE '.antigravity_tools\gui_config.json'),
    [switch]$Apply,
    [switch]$UseNativeGateway,
    [switch]$SelfTest
)

$ErrorActionPreference = 'Stop'
$script:Failures = 0
$script:Warnings = 0

function Write-Result {
    param([string]$Kind, [string]$Message)
    switch ($Kind) {
        'PASS' { Write-Host ("[PASS] " + $Message) -ForegroundColor Green }
        'WARN' { Write-Host ("[WARN] " + $Message) -ForegroundColor Yellow; $script:Warnings++ }
        'FAIL' { Write-Host ("[FAIL] " + $Message) -ForegroundColor Red; $script:Failures++ }
        default { Write-Host ("       " + $Message) }
    }
}

function Get-SecretFingerprint {
    param([string]$Value)
    if ([string]::IsNullOrEmpty($Value)) { return '<empty>' }
    $sha = [System.Security.Cryptography.SHA256]::Create()
    try {
        $bytes = $sha.ComputeHash([System.Text.Encoding]::UTF8.GetBytes($Value))
        $hex = -join ($bytes | ForEach-Object { $_.ToString('x2') })
        return ("len=" + $Value.Length + " sha12=" + $hex.Substring(0, 12))
    } finally {
        $sha.Dispose()
    }
}

function Get-TomlScalar {
    param([string[]]$Lines, [string]$Section, [string]$Key)
    $current = ''
    foreach ($line in $Lines) {
        $trimmed = $line.Trim()
        if ($trimmed -match '^\[(.+)\]$') { $current = $Matches[1]; continue }
        if ($current -ne $Section) { continue }
        if ($trimmed -match ('^' + [regex]::Escape($Key) + '\s*=\s*"(.*)"\s*$')) { return $Matches[1] }
    }
    return $null
}

function Set-TomlScalar {
    param([string[]]$Lines, [string]$Section, [string]$Key, [string]$Value)
    $output = New-Object System.Collections.Generic.List[string]
    $current = ''
    $done = $false
    foreach ($line in $Lines) {
        $trimmed = $line.Trim()
        if ($trimmed -match '^\[(.+)\]$') {
            if ($current -eq $Section -and -not $done) {
                $output.Add(('{0} = "{1}"' -f $Key, $Value))
                $done = $true
            }
            $current = $Matches[1]
            $output.Add($line)
            continue
        }
        if ($current -eq $Section -and -not $done -and $trimmed -match ('^' + [regex]::Escape($Key) + '\s*=')) {
            $output.Add(('{0} = "{1}"' -f $Key, $Value))
            $done = $true
            continue
        }
        $output.Add($line)
    }
    if ($current -eq $Section -and -not $done) {
        $output.Add(('{0} = "{1}"' -f $Key, $Value))
        $done = $true
    }
    return [pscustomobject]@{ Lines = $output.ToArray(); Updated = $done }
}

function Get-JsonField {
    param([object]$Object, [string]$Path)
    $node = $Object
    foreach ($part in $Path.Split('.')) {
        if ($null -eq $node -or -not ($node.PSObject.Properties.Name -contains $part)) { return $null }
        $node = $node.$part
    }
    return $node
}

function Set-JsonField {
    param([object]$Object, [string]$Path, [string]$Value)
    $parts = $Path.Split('.')
    $node = $Object
    foreach ($part in $parts[0..($parts.Count - 2)]) {
        if (-not ($node.PSObject.Properties.Name -contains $part) -or $null -eq $node.$part) {
            $node | Add-Member -NotePropertyName $part -NotePropertyValue ([pscustomobject]@{}) -Force
        }
        $node = $node.$part
    }
    $leaf = $parts[$parts.Count - 1]
    $node | Add-Member -NotePropertyName $leaf -NotePropertyValue $Value -Force
    return $Object
}

function Invoke-GatewayProbe {
    param([string]$Url, [string]$Token, [int]$TimeoutSec = 8)
    Add-Type -AssemblyName System.Net.Http
    $handler = New-Object System.Net.Http.HttpClientHandler
    $handler.UseProxy = $false
    $client = New-Object System.Net.Http.HttpClient($handler)
    $client.Timeout = [TimeSpan]::FromSeconds($TimeoutSec)
    $request = New-Object System.Net.Http.HttpRequestMessage ([System.Net.Http.HttpMethod]::Get), $Url
    if (-not [string]::IsNullOrEmpty($Token)) {
        $request.Headers.Authorization = New-Object System.Net.Http.Headers.AuthenticationHeaderValue 'Bearer', $Token
    }
    try {
        $response = $client.SendAsync($request).GetAwaiter().GetResult()
        $body = $response.Content.ReadAsStringAsync().GetAwaiter().GetResult()
        return [pscustomobject]@{ Status = [int]$response.StatusCode; Body = $body; Error = $null }
    } catch {
        return [pscustomobject]@{ Status = 0; Body = ''; Error = $_.Exception.GetBaseException().Message }
    } finally {
        $client.Dispose()
        $handler.Dispose()
    }
}

function ConvertTo-PrettyJson {
    param([object]$Object)
    $json = $Object | ConvertTo-Json -Depth 12
    $lines = $json -split "`r?`n"
    $out = @()
    foreach ($line in $lines) {
        if ($line -match '^(\s*)') {
            $level = [int]($Matches[1].Length / 4)
            $line = (' ' * (2 * $level)) + $line.TrimStart()
        }
        $out += ($line -replace '":\s{2}"', '": "')
    }
    return ($out -join "`n")
}

function Get-UserEnvironment {
    param([string]$Name)
    return [Environment]::GetEnvironmentVariable($Name, 'User')
}

function Test-ProxyBypass {
    param([string]$NoProxyValue)
    if ([string]::IsNullOrWhiteSpace($NoProxyValue)) { return $false }
    $entries = $NoProxyValue.Split(',') | ForEach-Object { $_.Trim().ToLowerInvariant() }
    return ($entries -contains '127.0.0.1') -and ($entries -contains 'localhost')
}

function Invoke-SelfTest {
    $root = Join-Path ([System.IO.Path]::GetTempPath()) ('codex-gateway-selftest-' + [guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Path $root | Out-Null
    try {
        $config = @(
            'model_provider = "OpenAI"',
            'model = "demo-model"',
            '',
            '[model_providers.OpenAI]',
            'name = "OpenAI"',
            'base_url = "http://127.0.0.1:8045/v1"',
            'experimental_bearer_token = "old-token"',
            '',
            '[projects.local]',
            'trust_level = "trusted"',
            'base_url = "http://127.0.0.1:8045/v1"'
        )
        $provider = Get-TomlScalar -Lines $config -Section '' -Key 'model_provider'
        if ($provider -ne 'OpenAI') { throw "root scalar parse failed: $provider" }
        $old = Get-TomlScalar -Lines $config -Section 'model_providers.OpenAI' -Key 'experimental_bearer_token'
        if ($old -ne 'old-token') { throw "section scalar parse failed: $old" }
        $updated = Set-TomlScalar -Lines $config -Section 'model_providers.OpenAI' -Key 'experimental_bearer_token' -Value 'new-token'
        if (-not $updated.Updated) { throw 'section update not reported' }
        $new = Get-TomlScalar -Lines $updated.Lines -Section 'model_providers.OpenAI' -Key 'experimental_bearer_token'
        if ($new -ne 'new-token') { throw "section update failed: $new" }
        $other = Get-TomlScalar -Lines $updated.Lines -Section 'projects.local' -Key 'base_url'
        if ($other -ne 'http://127.0.0.1:8045/v1') { throw "unrelated section mutated: $other" }
        if (-not ($updated.Lines -contains '# comment')) { } # comments above are preserved by construction

        $json = [pscustomobject]@{ proxy = [pscustomobject]@{ api_key = 'native-key' }; other = 1 }
        $key = Get-JsonField -Object $json -Path 'proxy.api_key'
        if ($key -ne 'native-key') { throw "json path read failed: $key" }
        Set-JsonField -Object $json -Path 'auth.token' -Value 'nested' | Out-Null
        if ((Get-JsonField -Object $json -Path 'auth.token') -ne 'nested') { throw 'json nested write failed' }

        if (-not (Test-ProxyBypass -NoProxyValue '127.0.0.1,localhost,::1')) { throw 'proxy bypass detection failed' }
        if (Test-ProxyBypass -NoProxyValue 'localhost') { throw 'proxy bypass false positive' }

        $fingerprint = Get-SecretFingerprint -Value 'sk-demo'
        if ($fingerprint -notmatch '^len=7 sha12=[0-9a-f]{12}$') { throw "fingerprint format: $fingerprint" }

        Write-Result PASS 'Self-test passed (TOML parse/update, JSON path, proxy bypass, fingerprint)'
        return 0
    } catch {
        Write-Result FAIL ("Self-test failed: " + $_.Exception.Message)
        return 1
    } finally {
        Remove-Item -Recurse -Force $root -ErrorAction SilentlyContinue
    }
}

if ($SelfTest) { exit (Invoke-SelfTest) }

# --- locate inputs -----------------------------------------------------------
$configPath = Join-Path $CodexHome 'config.toml'
$authPath = Join-Path $CodexHome 'auth.json'
$base = $GatewayBaseUrl.TrimEnd('/')
Write-Host ''
Write-Host 'Codex gateway doctor'
Write-Host ("  config : " + $configPath)
Write-Host ("  auth   : " + $authPath)
Write-Host ("  gateway: " + $base)
Write-Host ''

foreach ($path in @($configPath, $authPath)) {
    if (-not (Test-Path -LiteralPath $path)) { Write-Result FAIL ("missing file: " + $path) }
}
if ($script:Failures -gt 0) { exit 1 }

$configLines = Get-Content -LiteralPath $configPath -Encoding UTF8
$provider = Get-TomlScalar -Lines $configLines -Section '' -Key 'model_provider'
if ([string]::IsNullOrEmpty($provider)) { $provider = 'OpenAI' }
$providerSection = 'model_providers.' + $provider
Write-Result INFO ("provider section: " + $providerSection)

$configBaseUrl = Get-TomlScalar -Lines $configLines -Section $providerSection -Key 'base_url'
$configToken = Get-TomlScalar -Lines $configLines -Section $providerSection -Key 'experimental_bearer_token'
$auth = Get-Content -LiteralPath $authPath -Raw -Encoding UTF8 | ConvertFrom-Json
$authKey = $auth.OPENAI_API_KEY
$authBaseUrl = $auth.OPENAI_BASE_URL

$nativeKey = $null
if (Test-Path -LiteralPath $NativeGatewayConfig) {
    $native = Get-Content -LiteralPath $NativeGatewayConfig -Raw -Encoding UTF8 | ConvertFrom-Json
    $nativeKey = Get-JsonField -Object $native -Path 'proxy.api_key'
} else {
    Write-Result WARN ("native config not found: " + $NativeGatewayConfig)
}

# --- credential agreement ----------------------------------------------------
Write-Result INFO ("config token : " + (Get-SecretFingerprint $configToken))
Write-Result INFO ("auth token   : " + (Get-SecretFingerprint $authKey))
if ($nativeKey) { Write-Result INFO ("native token : " + (Get-SecretFingerprint $nativeKey)) }

if ([string]::IsNullOrEmpty($configToken)) {
    Write-Result WARN 'config has no experimental_bearer_token; Codex may fall back to auth.json'
} elseif ($configToken -ne $authKey) {
    Write-Result FAIL 'config.toml and auth.json bearer tokens differ'
} else {
    Write-Result PASS 'config.toml and auth.json bearer tokens match'
}
if ($nativeKey -and $nativeKey -ne $configToken) {
    Write-Result WARN 'Codex token differs from the native gateway key (expected when talking to the Docker relay)'
}
if ($configBaseUrl -ne $base) {
    Write-Result WARN ("config base_url is '" + $configBaseUrl + "', expected '" + $base + "'")
} else {
    Write-Result PASS 'config base_url points at the expected gateway'
}
if ($authBaseUrl -and $authBaseUrl -ne $base) {
    Write-Result WARN ("auth OPENAI_BASE_URL is '" + $authBaseUrl + "'")
}

# --- proxy bypass ------------------------------------------------------------
$userNoProxy = Get-UserEnvironment 'NO_PROXY'
Write-Result INFO ("user NO_PROXY: '" + $userNoProxy + "'")
if (Test-ProxyBypass $userNoProxy) {
    Write-Result PASS 'user NO_PROXY bypasses 127.0.0.1/localhost'
} else {
    Write-Result FAIL 'user NO_PROXY does not bypass 127.0.0.1/localhost'
}
$ie = Get-ItemProperty -Path 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Internet Settings'
if ($ie.ProxyEnable -eq 1) {
    Write-Result INFO ("system proxy enabled: " + $ie.ProxyServer)
    if (-not (Test-ProxyBypass $userNoProxy)) {
        Write-Result WARN 'Codex ignores ProxyOverride; without NO_PROXY local gateway calls go through the system proxy and fail with 502'
    }
} else {
    Write-Result INFO 'system proxy disabled'
}

# --- listener ownership ------------------------------------------------------
$uri = [uri]$base
$port = $uri.Port
$listeners = Get-NetTCPConnection -LocalPort $port -State Listen -ErrorAction SilentlyContinue
if (-not $listeners) {
    Write-Result FAIL ("nothing is listening on port " + $port)
} else {
    $owners = $listeners | ForEach-Object {
        $proc = Get-Process -Id $_.OwningProcess -ErrorAction SilentlyContinue
        [pscustomobject]@{ Address = $_.LocalAddress; Pid = $_.OwningProcess; Name = $proc.ProcessName }
    }
    foreach ($owner in $owners) {
        Write-Result INFO ("listen " + $owner.Address + ":" + $port + " pid=" + $owner.Pid + " " + $owner.Name)
    }
    $distinct = $owners | Select-Object -ExpandProperty Pid -Unique
    if ($distinct.Count -gt 1) {
        Write-Result WARN 'multiple processes share the port; which one accepts a connection is not guaranteed across reboots'
    }
    $established = Get-NetTCPConnection -LocalPort $port -State Established -ErrorAction SilentlyContinue
    if ($established) {
        $acceptedBy = $established | ForEach-Object {
            (Get-Process -Id $_.OwningProcess -ErrorAction SilentlyContinue).ProcessName
        } | Sort-Object -Unique
        Write-Result INFO ("current connections accepted by: " + ($acceptedBy -join ', '))
    }
}

# --- live probes -------------------------------------------------------------
$healthUrl = ($base -replace '/v1$', '') + '/health'
$health = Invoke-GatewayProbe -Url $healthUrl
if ($health.Status -eq 200) { Write-Result PASS 'GET /health -> 200' }
else { Write-Result FAIL ("GET /health -> " + $health.Status + " " + $health.Error) }

$anon = Invoke-GatewayProbe -Url ($base + '/models')
if ($anon.Status -eq 401) { Write-Result PASS 'unauthenticated GET /v1/models -> 401' }
else { Write-Result WARN ("unauthenticated GET /v1/models -> " + $anon.Status) }

$authed = Invoke-GatewayProbe -Url ($base + '/models') -Token $configToken
if ($authed.Status -eq 200) { Write-Result PASS 'authenticated GET /v1/models -> 200' }
else { Write-Result FAIL ("authenticated GET /v1/models -> " + $authed.Status + " " + $authed.Error) }

# --- repair ------------------------------------------------------------------
if ($Apply) {
    Write-Host ''
    Write-Host 'Applying repairs'
    $stamp = Get-Date -Format 'yyyyMMdd-HHmmss'
    $configBackup = $configPath + '.bak-gateway-' + $stamp
    $authBackup = $authPath + '.bak-gateway-' + $stamp
    Copy-Item -LiteralPath $configPath -Destination $configBackup
    Copy-Item -LiteralPath $authPath -Destination $authBackup
    Write-Result INFO ('backup: ' + $configBackup)
    Write-Result INFO ('backup: ' + $authBackup)

    $targetKey = $configToken
    if ($UseNativeGateway) {
        if ($nativeKey) { $targetKey = $nativeKey } else { Write-Result FAIL 'cannot use native gateway: key not found' }
    }
    if ([string]::IsNullOrEmpty($targetKey)) { Write-Result FAIL 'no usable token to apply' }

    if ($targetKey) {
        $utf8 = New-Object System.Text.UTF8Encoding($false)
        $rawConfig = [System.IO.File]::ReadAllText($configPath)
        $newline = if ($rawConfig.Contains("`r`n")) { "`r`n" } else { "`n" }
        $trailing = if ($rawConfig.EndsWith("`n")) { $newline } else { '' }
        $configLines = Get-Content -LiteralPath $configPath -Encoding UTF8
        $urlUpdate = Set-TomlScalar -Lines $configLines -Section $providerSection -Key 'base_url' -Value $base
        $keyUpdate = Set-TomlScalar -Lines $urlUpdate.Lines -Section $providerSection -Key 'experimental_bearer_token' -Value $targetKey
        [System.IO.File]::WriteAllText($configPath, ($keyUpdate.Lines -join $newline) + $trailing, $utf8)

        $auth | Add-Member -NotePropertyName OPENAI_API_KEY -NotePropertyValue $targetKey -Force
        $auth | Add-Member -NotePropertyName OPENAI_BASE_URL -NotePropertyValue $base -Force
        $authText = ConvertTo-PrettyJson -Object $auth
        [System.IO.File]::WriteAllText($authPath, ($authText + $newline), $utf8)
        Write-Result PASS 'config.toml and auth.json updated'
    }

    if (-not (Test-ProxyBypass (Get-UserEnvironment 'NO_PROXY'))) {
        [Environment]::SetEnvironmentVariable('NO_PROXY', '127.0.0.1,localhost,::1', 'User')
        Write-Result PASS 'persisted NO_PROXY=127.0.0.1,localhost,::1 for the current user'
    }

    $script:Failures = 0
    $script:Warnings = 0
    Write-Result INFO 'pre-repair findings above are fixed by the applied changes'
    $verify = Invoke-GatewayProbe -Url ($base + '/models') -Token $targetKey
    if ($verify.Status -eq 200) { Write-Result PASS 'post-repair authenticated GET /v1/models -> 200' }
    else { Write-Result FAIL ("post-repair probe -> " + $verify.Status) }
}

Write-Host ''
Write-Host ("Summary: " + $script:Failures + " failure(s), " + $script:Warnings + " warning(s)")
if (-not $Apply) { Write-Host 'Run again with -Apply (and optionally -UseNativeGateway) to repair.' }
if ($script:Failures -gt 0) { exit 1 }
exit 0
