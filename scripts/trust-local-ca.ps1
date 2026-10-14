param([Parameter(Mandatory = $true)][string]$CertificatePath)
$ErrorActionPreference = 'Stop'
# Import the public root only, scoped to this Windows user. Never copy a CA key.
$certificate = New-Object System.Security.Cryptography.X509Certificates.X509Certificate2($CertificatePath)
$store = New-Object System.Security.Cryptography.X509Certificates.X509Store('Root', 'CurrentUser')
try {
    $store.Open('ReadWrite')
    $found = $store.Certificates.Find('FindByThumbprint', $certificate.Thumbprint, $false)
    if ($found.Count -eq 0) { $store.Add($certificate) }
} finally {
    $store.Close()
    $certificate.Dispose()
}
Write-Output 'Local HTTPS CA is trusted by the current Windows user.'
