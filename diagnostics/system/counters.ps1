# Author: libs (hle-d3d12-7900xtx)
# Collects system and process counters for victory.exe every $Every seconds into a CSV (and a thread CSV).
# Usage: powershell -ExecutionPolicy Bypass -File counters.ps1 -Seconds 300 -Out <dir>
param([int]$Seconds = 300, [int]$Every = 2, [string]$Out = "$PWD\counters")
New-Item -ItemType Directory -Force $Out | Out-Null
$stamp = Get-Date -Format 'yyyyMMdd-HHmmss'
$main = Join-Path $Out "counters-$stamp.csv"
$thr = Join-Path $Out "threads-$stamp.csv"
"time,cpu_total_pct,cores_busy_over_80,proc_cpu_pct,threads,page_faults_s,working_set_mb,private_mb,commit_total_mb,gpu_3d_pct,gpu_compute_pct,gpu_copy_pct,gpu_dedicated_mb,gpu_shared_mb" | Set-Content $main -Encoding ascii
"time,tid,cpu_ms_delta,state,wait_reason" | Set-Content $thr -Encoding ascii
$cores = [Environment]::ProcessorCount
$prevThreads = @{}
$prevProc = $null
$end = (Get-Date).AddSeconds($Seconds)
while ((Get-Date) -lt $end) {
    $p = Get-Process victory -ErrorAction SilentlyContinue | Sort-Object WorkingSet64 -Descending | Select-Object -First 1
    if (-not $p) { Start-Sleep -Seconds $Every; continue }
    $now = Get-Date
    $t = $now.ToString('HH:mm:ss')
    # Per-core and total CPU, and the process's page faults (WMI classes have English names on any locale).
    $cpu = Get-CimInstance Win32_PerfFormattedData_PerfOS_Processor
    $total = ($cpu | Where-Object Name -eq '_Total').PercentProcessorTime
    $hot = ($cpu | Where-Object { $_.Name -ne '_Total' -and $_.PercentProcessorTime -gt 80 }).Count
    $pp = Get-CimInstance Win32_PerfFormattedData_PerfProc_Process -Filter "IDProcess=$($p.Id)"
    $os = Get-CimInstance Win32_OperatingSystem
    $commit = [math]::Round(($os.TotalVirtualMemorySize - $os.FreeVirtualMemory) / 1024)
    # GPU engines and memory for this process (counter names are English).
    $g3d = 0; $gcomp = 0; $gcopy = 0; $ded = 0; $sh = 0
    try {
        $gs = (Get-Counter "\GPU Engine(pid_$($p.Id)*)\Utilization Percentage" -ErrorAction Stop).CounterSamples
        $g3d = ($gs | Where-Object InstanceName -like '*engtype_3d' | Measure-Object CookedValue -Sum).Sum
        $gcomp = ($gs | Where-Object InstanceName -like '*engtype_compute*' | Measure-Object CookedValue -Sum).Sum
        $gcopy = ($gs | Where-Object InstanceName -like '*engtype_copy' | Measure-Object CookedValue -Sum).Sum
        $gm = (Get-Counter "\GPU Process Memory(pid_$($p.Id)*)\Dedicated Usage", "\GPU Process Memory(pid_$($p.Id)*)\Shared Usage" -ErrorAction Stop).CounterSamples
        $ded = [math]::Round((($gm | Where-Object Path -like '*dedicated usage' | Measure-Object CookedValue -Sum).Sum) / 1MB)
        $sh = [math]::Round((($gm | Where-Object Path -like '*shared usage' | Measure-Object CookedValue -Sum).Sum) / 1MB)
    } catch {}
    # Process CPU over the interval, in percent of ONE core.
    $procPct = 0
    if ($prevProc) { $procPct = [math]::Round(100 * ($p.TotalProcessorTime - $prevProc.cpu).TotalMilliseconds / ($now - $prevProc.at).TotalMilliseconds) }
    $prevProc = @{ cpu = $p.TotalProcessorTime; at = $now }
    "{0},{1},{2},{3},{4},{5},{6},{7},{8},{9:N1},{10:N1},{11:N1},{12},{13}" -f $t, $total, $hot, $procPct, $p.Threads.Count, $pp.PageFaultsPersec,
        [math]::Round($p.WorkingSet64 / 1MB), [math]::Round($p.PrivateMemorySize64 / 1MB), $commit, $g3d, $gcomp, $gcopy, $ded, $sh | Add-Content $main -Encoding ascii
    # Per-thread CPU over the interval.
    $lines = foreach ($th in $p.Threads) {
        try {
            $ms = $th.TotalProcessorTime.TotalMilliseconds
            $d = if ($prevThreads.ContainsKey($th.Id)) { $ms - $prevThreads[$th.Id] } else { 0 }
            $prevThreads[$th.Id] = $ms
            if ($d -gt 0) { "$t,$($th.Id),$([math]::Round($d)),$($th.ThreadState),$($th.WaitReason)" }
        } catch {}
    }
    if ($lines) { $lines | Add-Content $thr -Encoding ascii }
    Start-Sleep -Seconds $Every
}
Write-Host "Counters: $main"
Write-Host "Threads:  $thr"
