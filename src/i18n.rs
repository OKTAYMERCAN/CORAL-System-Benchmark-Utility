//! English / Turkish text. Keys are looked up at render time, so a saved result file can be
//! re-rendered in either language.

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Lang {
    En,
    Tr,
}

impl Lang {
    pub fn parse(s: &str) -> Option<Lang> {
        match s.trim().to_lowercase().as_str() {
            "en" | "eng" | "english" | "1" => Some(Lang::En),
            "tr" | "tur" | "turkce" | "türkçe" | "turkish" | "2" => Some(Lang::Tr),
            _ => None,
        }
    }
}

/// Look up a key. Unknown keys are returned unchanged so a missing translation is visible.
pub fn t<'a>(lang: Lang, key: &'a str) -> &'a str {
    for (k, en, tr) in TABLE {
        if *k == key {
            return if lang == Lang::Tr { tr } else { en };
        }
    }
    key
}

/// Look up a key and substitute `{0}`, `{1}`, ... with `args`.
pub fn tf(lang: Lang, key: &str, args: &[String]) -> String {
    let mut s = t(lang, key).to_string();
    for (i, a) in args.iter().enumerate() {
        s = s.replace(&format!("{{{i}}}"), a);
    }
    s
}

pub static TABLE: &[(&str, &str, &str)] = &[
    // progress messages
    ("prog_cpu_single", "CPU single-core", "CPU tek çekirdek"),
    ("prog_cpu_multi", "CPU multi-core", "CPU çok çekirdek"),
    ("prog_cpu_groups", "CPU core clusters", "CPU çekirdek kümeleri"),
    ("prog_cpu_accel", "CPU hardware acceleration", "CPU donanım hızlandırma"),
    ("prog_mem_bw", "Memory bandwidth", "Bellek bant genişliği"),
    ("prog_mem_lat", "Memory latency", "Bellek gecikmesi"),
    ("prog_disk_prepare", "Disk: preparing test file", "Disk: test dosyası hazırlanıyor"),
    ("prog_disk_sweep", "Disk block-size sweep", "Disk blok boyutu taraması"),
    ("prog_disk_scen", "Disk scenario", "Disk senaryosu"),
    ("prog_disk_small", "Disk: small files", "Disk: küçük dosyalar"),
    ("prog_disk_sustained", "Disk sustained write", "Disk sürekli yazma"),
    ("prog_net_tcp", "Network TCP throughput", "Ağ TCP verimi"),
    ("prog_net_rtt", "Network round-trip time", "Ağ gidiş-dönüş süresi"),
    ("prog_net_sweep", "Network message sizes", "Ağ mesaj boyutları"),
    ("prog_net_gw", "Gateway round-trip time", "Ağ geçidi gecikmesi"),
    ("yes", "yes", "evet"),
    ("no", "no", "hayır"),
    ("unknown", "unknown", "bilinmiyor"),
    ("none", "none", "yok"),
    ("h_system", "System", "Sistem"),
    ("h_cpu", "CPU", "CPU (işlemci)"),
    ("h_mem", "Memory", "Bellek"),
    ("h_disk", "Disk", "Disk"),
    ("h_net", "Network", "Ağ"),
    ("h_scores", "Suitability scores", "Uygunluk puanları"),
    ("h_findings", "Findings", "Bulgular"),
    ("cat_virt", "Virtualization host (VMs)", "Sanallaştırma sunucusu (VM)"),
    ("cat_nas", "NAS / file server", "NAS / dosya sunucusu"),
    ("cat_video", "Media / video server", "Medya / video sunucusu"),
    ("cat_game", "Game server", "Oyun sunucusu"),
    ("cat_ai", "AI / LLM (CPU only)", "Yapay zeka / LLM (yalnız CPU)"),
    ("cat_gfx", "Graphics (GPU)", "Grafik (GPU)"),
    ("cat_rt", "Ray tracing", "Işın izleme (ray tracing)"),
    ("cat_general", "General performance", "Genel performans"),
    ("cmp_cpu_single", "CPU single-core", "CPU tek çekirdek"),
    ("cmp_cpu_multi", "CPU multi-core", "CPU çok çekirdek"),
    ("cmp_cpu_fp", "CPU floating point", "CPU kayan nokta"),
    ("cmp_crypto", "Hashing / crypto", "Özet / şifreleme"),
    ("cmp_ram", "RAM size", "RAM miktarı"),
    ("cmp_mem_bw", "Memory bandwidth", "Bellek bant genişliği"),
    ("cmp_mem_lat", "Memory latency", "Bellek gecikmesi"),
    ("cmp_disk_seq", "Disk sequential", "Disk ardışık"),
    ("cmp_disk_iops", "Disk random IOPS (QD32)", "Disk rastgele IOPS (QD32)"),
    ("cmp_disk_qd1", "Disk random QD1", "Disk rastgele QD1"),
    ("cmp_streams", "Disk parallel streams", "Disk paralel akışlar"),
    ("cmp_net", "Network throughput", "Ağ verimi"),
    ("fc_general", "general", "genel"),
    ("fc_virt", "virtualization", "sanallaştırma"),
    ("fc_nas", "NAS", "NAS"),
    ("fc_video", "video", "video"),
    ("fc_game", "game", "oyun"),
    ("fc_ai", "AI", "yapay zeka"),
    ("fc_gpu", "GPU", "GPU"),
    ("fc_disk", "disk", "disk"),
    ("fc_net", "network", "ağ"),
    ("ver_excellent", "excellent", "mükemmel"),
    ("ver_good", "good", "iyi"),
    ("ver_limited", "limited", "sınırlı"),
    ("ver_poor", "poor", "zayıf"),
    ("k_int_alu", "Integer ALU", "Tamsayı ALU"),
    ("k_fp_f64", "Floating point (f64)", "Kayan nokta (f64)"),
    ("k_matmul_f32", "Matrix multiply (f32)", "Matris çarpımı (f32)"),
    ("k_sort_u32", "Sort (u32)", "Sıralama (u32)"),
    ("k_sha256_soft", "SHA-256 (software)", "SHA-256 (yazılım)"),
    ("k_deflate", "Compression (deflate)", "Sıkıştırma (deflate)"),
    ("s_rand4k_read_qd1", "4K random read, QD1", "4K rastgele okuma, QD1"),
    ("s_rand4k_read_qd32", "4K random read, QD32", "4K rastgele okuma, QD32"),
    ("s_rand4k_write_qd32", "4K random write, QD32", "4K rastgele yazma, QD32"),
    ("s_mixed8k_70r30w_qd32", "8K mixed 70/30, QD32", "8K karışık 70/30, QD32"),
    ("s_seq1m_read_qd4", "1 MB sequential read, QD4", "1 MB ardışık okuma, QD4"),
    ("s_seq1m_write_qd4", "1 MB sequential write, QD4", "1 MB ardışık yazma, QD4"),
    ("u_cores", "cores", "çekirdek"),
    ("u_threads", "threads", "iş parçacığı"),
    ("u_thread", "thread", "iş parçacığı"),
    ("u_free", "free", "boş"),
    ("u_samples", "samples", "örnek"),
    ("l_label", "Device name", "Cihaz adı"),
    ("l_model", "Model", "Model"),
    ("l_os", "System", "Sistem"),
    ("l_android", "Android", "Android"),
    ("l_container", "Container", "Konteyner"),
    ("l_cpu", "CPU", "CPU"),
    ("l_cores", "Cores", "Çekirdekler"),
    ("l_cluster", "Core cluster", "Çekirdek kümesi"),
    ("l_freq", "Max frequency", "En yüksek frekans"),
    ("l_cache", "Cache", "Önbellek"),
    ("l_features", "CPU features", "CPU özellikleri"),
    ("l_virt", "Virtualization", "Sanallaştırma"),
    ("l_hypervisor", "Runs inside a VM", "Bir VM içinde çalışıyor"),
    ("l_ram", "RAM", "RAM"),
    ("l_disk", "Disk", "Disk"),
    ("l_nic", "Network", "Ağ"),
    ("l_gpu", "GPU", "GPU"),
    ("l_hwvideo", "HW video encoders", "Donanım video kodlayıcı"),
    ("l_kernel", "Test", "Test"),
    ("l_single", "1 core", "1 çekirdek"),
    ("l_multi", "All cores", "Tüm çekirdekler"),
    ("l_geo", "Geometric mean", "Geometrik ortalama"),
    ("l_scaling", "Multi-core scaling", "Çok çekirdek ölçekleme"),
    ("l_noise", "Max. run-to-run noise", "En yüksek ölçüm dalgalanması"),
    ("l_pinned", "Threads pinned to cores", "İş parçacıkları çekirdeğe sabitlendi"),
    ("l_sha", "SHA-256 soft → hardware", "SHA-256 yazılım → donanım"),
    ("l_simd", "SIMD (matrix) base → accelerated", "SIMD (matris) temel → hızlandırılmış"),
    ("l_clusters", "Per core cluster", "Çekirdek kümesi başına"),
    ("l_op", "Operation", "İşlem"),
    ("l_read", "Read", "Okuma"),
    ("l_write", "Write", "Yazma"),
    ("l_copy", "Copy", "Kopyalama"),
    ("l_triad", "Triad", "Triad"),
    ("l_latency", "Memory latency by working-set size", "Çalışma kümesi boyutuna göre bellek gecikmesi"),
    ("l_io_mode", "I/O mode", "G/Ç modu"),
    ("l_testfile", "Test file", "Test dosyası"),
    ("l_block", "logical block", "mantıksal blok"),
    ("l_sweep", "Block-size sweep (sequential)", "Blok boyutu taraması (ardışık)"),
    ("l_scenarios", "Real-world scenarios", "Gerçek kullanım senaryoları"),
    ("l_scenario", "Scenario", "Senaryo"),
    ("l_streams", "Video streams", "Video akışları"),
    ("l_smallfiles", "Small files", "Küçük dosyalar"),
    ("l_sustained", "Sustained write (MB/s per second)", "Sürekli yazma (saniye başına MB/s)"),
    ("l_net_mode", "Test type", "Test türü"),
    ("l_up1", "Upload, 1 stream", "Yükleme, 1 akış"),
    ("l_up4", "Upload, 4 streams", "Yükleme, 4 akış"),
    ("l_down1", "Download, 1 stream", "İndirme, 1 akış"),
    ("l_down4", "Download, 4 streams", "İndirme, 4 akış"),
    ("l_bidir", "Both directions", "Çift yönlü"),
    ("l_link", "Link speed", "Bağlantı hızı"),
    ("l_ceiling", "practical ceiling", "pratik tavan"),
    ("l_rtt", "Round trip (server)", "Gidiş-dönüş (sunucu)"),
    ("l_jitter", "jitter", "titreşim"),
    ("l_gateway", "Round trip (gateway)", "Gidiş-dönüş (ağ geçidi)"),
    ("l_msgsweep", "Echo throughput by message size", "Mesaj boyutuna göre yankı verimi"),
    ("l_components", "Score building blocks (0-100)", "Puan bileşenleri (0-100)"),
    ("msg_subtitle", "system benchmark", "sistem performans testi"),
    ("msg_footer", "Scores are provisional (log scale) and will be recalibrated with collected results; raw values are stored in the result file. Compare results only between runs of the same spec version.", "Puanlar geçicidir (logaritmik ölçek), toplanan sonuçlarla yeniden ayarlanacaktır; ham değerler sonuç dosyasında saklanır. Sonuçları yalnızca aynı spec sürümündeki çalıştırmalar arasında karşılaştırın."),
    ("msg_no_hw", "no hardware SHA", "donanım SHA yok"),
    ("msg_no_simd", "no wider SIMD detected (baseline only)", "daha geniş SIMD bulunamadı (yalnız temel)"),
    ("msg_array", "Test arrays: {0} MB each (larger than the CPU cache)", "Test dizileri: her biri {0} MB (CPU önbelleğinden büyük)"),
    ("msg_lat_summary", "L1 hit {0} ns · main memory {1} ns", "L1 isabeti {0} ns · ana bellek {1} ns"),
    ("msg_disk_error", "Disk test failed", "Disk testi başarısız"),
    ("msg_direct", "direct (unbuffered) I/O", "doğrudan (önbelleksiz) G/Ç"),
    ("msg_buffered", "buffered I/O: direct I/O not available here, numbers may include cache", "önbellekli G/Ç: burada doğrudan G/Ç yok, sayılar önbellek içerebilir"),
    ("msg_qd_capped", "limited to", "sınırlandı:"),
    ("msg_streams", "{0} parallel 256 KB streams: {1} MB/s total, slowest {2} MB/s → about {3} 4K video streams (10 MB/s each)", "{0} paralel 256 KB akış: toplam {1} MB/s, en yavaş {2} MB/s → yaklaşık {3} adet 4K video akışı (her biri 10 MB/s)"),
    ("msg_smallfiles", "{0} files of 4 KB: create {1}/s · read {2}/s · delete {3}/s", "{0} adet 4 KB dosya: oluşturma {1}/sn · okuma {2}/sn · silme {3}/sn"),
    ("msg_sustained", "start {0} MB/s → end {1} MB/s (min {2} MB/s), drop {3} %", "başlangıç {0} MB/s → son {1} MB/s (en düşük {2} MB/s), düşüş %{3}"),
    ("msg_net_error", "Network test failed", "Ağ testi başarısız"),
    ("msg_net_remote", "real network (against a SysCheck server)", "gerçek ağ (bir SysCheck sunucusuna karşı)"),
    ("msg_net_loopback", "loopback only: measures this machine's TCP stack, not the network", "yalnız loopback: ağı değil, bu makinenin TCP yığınını ölçer"),
    ("msg_loopback_note", "Run `syscheck serve` on another machine and use --net-server IP to measure the real network.", "Gerçek ağı ölçmek için başka bir makinede `syscheck serve` çalıştırıp --net-server IP kullanın."),
    ("msg_not_measured", "not measured", "ölçülmedi"),
    ("msg_gpu_planned", "not measured yet (GPU tests are planned)", "henüz ölçülmedi (GPU testleri planlanıyor)"),
    ("msg_debug_warn", "Warning: this is a debug build; results will be far too low. Use `cargo build --release`.", "Uyarı: bu bir debug derlemesi; sonuçlar çok düşük çıkar. `cargo build --release` kullanın."),
    ("msg_no_tests", "No valid test selected. Use --only cpu,mem,disk,net,video,rt,gpu", "Geçerli test seçilmedi. --only cpu,mem,disk,net,video,rt,gpu kullanın"),
    ("msg_detecting", "Detecting hardware…", "Donanım algılanıyor…"),
    ("msg_done", "done", "tamam"),
    ("msg_save_failed", "Could not save results", "Sonuçlar kaydedilemedi"),
    ("msg_saved", "Saved: {0}", "Kaydedildi: {0}"),
    ("msg_csv_added", "CSV row added: {0}", "CSV satırı eklendi: {0}"),
    ("fnd_debug_build", "Debug build: numbers are not meaningful. Rebuild with --release.", "Debug derlemesi: sayılar anlamlı değil. --release ile yeniden derleyin."),
    ("fnd_virt_ok", "Hardware virtualization is available (VT-x / AMD-V / KVM).", "Donanım sanallaştırma mevcut (VT-x / AMD-V / KVM)."),
    ("fnd_virt_none", "No hardware virtualization: enable VT-x / AMD-V (SVM) in BIOS/UEFI. Virtual machines need it.", "Donanım sanallaştırma yok: BIOS/UEFI içinde VT-x / AMD-V (SVM) açın. Sanal makineler buna ihtiyaç duyar."),
    ("fnd_virt_guest_nested", "Running inside a VM without nested virtualization: enable nested virtualization on the host to run VMs here.", "Nested sanallaştırma olmayan bir VM içinde çalışıyor: burada VM çalıştırmak için ana makinede nested sanallaştırmayı açın."),
    ("fnd_virt_unknown", "Could not determine hardware virtualization support.", "Donanım sanallaştırma desteği belirlenemedi."),
    ("fnd_iommu_yes", "IOMMU active ({0} groups): PCIe passthrough (GPU, HBA) is possible.", "IOMMU etkin ({0} grup): PCIe passthrough (GPU, HBA) mümkün."),
    ("fnd_iommu_no", "IOMMU not active or not visible: PCIe passthrough may be unavailable (check BIOS: VT-d / AMD-Vi).", "IOMMU etkin değil veya görünmüyor: PCIe passthrough kullanılamayabilir (BIOS: VT-d / AMD-Vi)."),
    ("fnd_ram_low_vm", "Only {0} GB RAM: too little for more than a couple of small VMs.", "Yalnızca {0} GB RAM: birkaç küçük VM'den fazlası için yetersiz."),
    ("fnd_ecc_yes", "ECC memory detected: good for NAS data integrity.", "ECC bellek algılandı: NAS veri bütünlüğü için iyi."),
    ("fnd_no_ecc", "No ECC memory: acceptable for home use, ECC is recommended for important NAS data.", "ECC bellek yok: ev kullanımı için kabul edilebilir, önemli NAS verisi için ECC önerilir."),
    ("fnd_noise_high", "Measurements fluctuated by up to {0} %: other load or thermal throttling; results are less reliable.", "Ölçümler %{0} kadar dalgalandı: başka yük veya ısıl kısıtlama olabilir; sonuçlar daha az güvenilir."),
    ("fnd_not_pinned", "Threads could not be pinned to cores (restricted environment): single-core numbers may vary.", "İş parçacıkları çekirdeğe sabitlenemedi (kısıtlı ortam): tek çekirdek sayıları değişebilir."),
    ("fnd_sha_hw", "Hardware SHA ({0}) is {1}× faster than software: good for checksums and encryption.", "Donanım SHA ({0}) yazılımdan {1}× hızlı: sağlama toplamı ve şifreleme için iyi."),
    ("fnd_simd", "SIMD acceleration ({0}) gives {1}× in the matrix test: helps AI inference on the CPU.", "SIMD hızlandırma ({0}) matris testinde {1}× kazanç sağlıyor: CPU üzerinde yapay zeka çıkarımına yardımcı olur."),
    ("fnd_hybrid_cpu", "Hybrid CPU with {0} core types: the scheduler decides which cores run your VMs and transcodes.", "{0} çekirdek türüne sahip hibrit CPU: VM ve dönüştürmelerin hangi çekirdeklerde çalışacağına zamanlayıcı karar verir."),
    ("fnd_disk_ram", "The test directory is RAM-backed (tmpfs/ramdisk): disk numbers show memory speed, not a drive. Use --path on a real disk.", "Test dizini RAM üzerinde (tmpfs/ramdisk): disk sayıları sürücüyü değil bellek hızını gösterir. Gerçek bir diskte --path kullanın."),
    ("fnd_disk_hdd", "Spinning hard disk: poor for VMs (random I/O); fine for bulk NAS storage.", "Dönen sabit disk: VM'ler için zayıf (rastgele G/Ç); toplu NAS depolama için uygun."),
    ("fnd_disk_error", "Disk test could not run: {0}", "Disk testi çalışamadı: {0}"),
    ("fnd_disk_buffered", "Direct I/O was not available, so disk numbers may include the OS cache.", "Doğrudan G/Ç kullanılamadı, bu yüzden disk sayıları işletim sistemi önbelleğini içerebilir."),
    ("fnd_disk_streams", "The disk can feed about {0} parallel 4K video streams.", "Disk yaklaşık {0} paralel 4K video akışını besleyebilir."),
    ("fnd_sustained_drop", "Sustained writes drop by {0} % (cache exhausted or thermal limit): large transfers will slow down.", "Sürekli yazma %{0} düşüyor (önbellek doldu veya ısıl sınır): büyük aktarımlar yavaşlayacak."),
    ("fnd_sustained_ok", "Sustained write speed is stable (drop {0} %).", "Sürekli yazma hızı kararlı (düşüş %{0})."),
    ("fnd_net_error", "Network test failed: {0}", "Ağ testi başarısız: {0}"),
    ("fnd_net_loopback", "Network was tested on loopback only: no real network figure. Use `syscheck serve` on another machine.", "Ağ yalnız loopback üzerinde test edildi: gerçek ağ değeri yok. Başka bir makinede `syscheck serve` kullanın."),
    ("fnd_net_link_slow", "Network link is only {0} Mbit/s: slow for a NAS (1 GbE ≈ 110 MB/s).", "Ağ bağlantısı yalnızca {0} Mbit/s: NAS için yavaş (1 GbE ≈ 110 MB/s)."),
    ("fnd_net_rtt_high", "High network latency (p99 {0} ms).", "Yüksek ağ gecikmesi (p99 {0} ms)."),
    ("fnd_net_eff_low", "Network reaches only {0} % of what the link can carry: check cable, switch, NIC settings or CPU.", "Ağ, bağlantının taşıyabileceğinin yalnızca %{0}'ine ulaşıyor: kablo, switch, NIC ayarları veya CPU'yu kontrol edin."),
    ("fnd_net_ok", "Network works well: {0} Mbit/s ({1} % of the link).", "Ağ iyi çalışıyor: {0} Mbit/s (bağlantının %{1}'i)."),
    ("fnd_gpu_none", "No GPU detected.", "GPU algılanmadı."),
    ("fnd_gpu_found", "GPU found: {0} (the GPU tests were not run; use --only gpu).", "GPU bulundu: {0} (GPU testleri çalıştırılmadı; --only gpu kullanın)."),
    ("fnd_hw_video", "Hardware video encoders detected ({0}); not tested (use --only video).", "Donanım video kodlayıcılar algılandı ({0}); test edilmedi (--only video kullanın)."),
    ("fnd_hw_video_none", "No hardware video encoder detected: transcoding runs on the CPU.", "Donanım video kodlayıcı algılanmadı: dönüştürme CPU üzerinde yapılır."),
    ("msg_net_howto", "Network not tested (by default only the machine itself is measured). A real network test needs two machines: run `syscheck serve` on one, then `syscheck net --server IP` on the other. The result is limited by the weaker of the two devices (CPU, network card, cable, Wi-Fi), so a slow device lowers the number. `--net-loopback` runs a local-only test; it measures this machine's TCP stack, not the network.", "Ağ test edilmedi (varsayılan olarak yalnız makinenin kendisi ölçülür). Gerçek ağ testi için iki makine gerekir: birinde `syscheck serve`, diğerinde `syscheck net --server IP` çalıştırın. Sonuç, iki cihazdan zayıf olanla (CPU, ağ kartı, kablo, Wi-Fi) sınırlıdır; yani yavaş bir cihaz değeri düşürür. `--net-loopback` yalnızca yerel bir test yapar; ağı değil, bu makinenin TCP yığınını ölçer."),
    ("msg_net_need_server","The network test needs --net-server IP (real network) or --net-loopback (local stack only).","Ağ testi için --net-server IP (gerçek ağ) veya --net-loopback (yalnız yerel yığın) gerekir."),
    ("fnd_net_not_tested","Network was not tested, so the NAS score has no network part. See the network section for how to test it.","Ağ test edilmedi, bu yüzden NAS puanında ağ bileşeni yok. Nasıl test edileceği için ağ bölümüne bakın."),
    ("prog_video", "Video encode", "Video kodlama"),
    ("prog_rt", "Ray tracing (path tracer)", "Işın izleme (path tracer)"),
    ("l_checksum", "reference check", "referans kontrolü"),
    ("prog_gpu", "GPU", "GPU"),
    ("l_gpu_detect", "detecting adapters", "adaptörler algılanıyor"),
    ("h_video", "Video encoding", "Video kodlama"),
    ("h_rt", "Ray tracing", "Işın izleme"),
    ("h_gpu", "GPU", "GPU"),
    ("l_ffmpeg", "ffmpeg version", "ffmpeg sürümü"),
    ("l_source_fps", "Test source speed", "Test kaynağı hızı"),
    ("l_encoder", "Encoder", "Kodlayıcı"),
    ("l_codec", "Codec", "Codec"),
    ("l_rt_streams", "1080p30 streams", "1080p30 akış"),
    ("msg_enc_unavailable", "not working on this machine", "bu makinede çalışmıyor"),
    ("msg_ffmpeg_missing", "ffmpeg was not found, so video encoding was not tested. Install ffmpeg to test CPU and hardware encoders.", "ffmpeg bulunamadı, bu yüzden video kodlama test edilmedi. CPU ve donanım kodlayıcıları test etmek için ffmpeg kurun."),
    ("msg_video_note", "Fixed 1080p30 H.264/HEVC test source. Streams = how many real-time (30 fps) encodes fit. * = close to the speed of the test source itself, the real limit may be higher.", "Sabit 1080p30 H.264/HEVC test kaynağı. Akış = kaç adet gerçek zamanlı (30 fps) kodlama sığar. * = test kaynağının kendi hızına yakın, gerçek sınır daha yüksek olabilir."),
    ("l_pt_scene", "Test scene", "Test sahnesi"),
    ("u_prims", "objects", "nesne"),
    ("u_bounces", "bounces", "sekme"),
    ("l_cpu_pt1", "CPU, 1 thread", "CPU, 1 iş parçacığı"),
    ("l_cpu_ptall", "CPU, all threads", "CPU, tüm iş parçacıkları"),
    ("l_gpu_pt", "GPU (compute shaders)", "GPU (hesaplama gölgelendiricileri)"),
    ("msg_checksum_ok", "{0}  ✔ identical to the reference", "{0}  ✔ referansla aynı"),
    ("msg_checksum_bad", "{0}  ✘ differs from the reference", "{0}  ✘ referanstan farklı"),
    ("msg_software_short", "software", "yazılım"),
    ("msg_vs_cpu", "vs CPU", "CPU'ya göre"),
    ("msg_rt_note", "Same fixed path-traced scene on CPU and GPU (shader cores). Dedicated ray-tracing units are only detected, not measured yet.", "CPU ve GPU'da aynı sabit path-tracing sahnesi (gölgelendirici çekirdekleri). Özel ışın izleme birimleri yalnızca algılanır, henüz ölçülmüyor."),
    ("l_fp32", "FP32 throughput", "FP32 hızı"),
    ("l_int", "Integer throughput", "Tamsayı hızı"),
    ("l_membw", "Memory bandwidth", "Bellek bant genişliği"),
    ("l_matmul", "Matrix multiply", "Matris çarpımı"),
    ("l_transfer", "Host ↔ GPU copy", "Ana bellek ↔ GPU kopyası"),
    ("l_dispatch", "Submit round trip", "Gönderim gidiş-dönüşü"),
    ("msg_gpu_none", "No usable GPU adapter was found (no graphics driver, or running in a container/VM without GPU access).", "Kullanılabilir GPU adaptörü bulunamadı (grafik sürücüsü yok, ya da GPU erişimi olmayan bir konteyner/VM içinde)."),
    ("msg_gpu_not_built", "This build has no GPU support (built with --no-default-features).", "Bu derlemede GPU desteği yok (--no-default-features ile derlenmiş)."),
    ("msg_gpu_failed", "GPU test failed", "GPU testi başarısız"),
    ("msg_gpu_software", "Software renderer (runs on the CPU): not a real GPU, its numbers are not scored.", "Yazılım işleyicisi (CPU üzerinde çalışır): gerçek bir GPU değil, değerleri puanlanmaz."),
    ("msg_hw_rt_detected", "The driver reports hardware ray-tracing support (detected only, not measured yet).", "Sürücü donanım ışın izleme desteği bildiriyor (yalnızca algılandı, henüz ölçülmedi)."),
    ("fnd_ffmpeg_missing", "ffmpeg not found: video encoding was not tested.", "ffmpeg bulunamadı: video kodlama test edilmedi."),
    ("fnd_video_hw_ok", "Hardware encoder {0} works: {1} fps, about {2} real-time 1080p30 streams.", "Donanım kodlayıcı {0} çalışıyor: {1} fps, yaklaşık {2} gerçek zamanlı 1080p30 akış."),
    ("fnd_video_source_limited", "The hardware encoder is as fast as the test source, so its real speed may be higher.", "Donanım kodlayıcı test kaynağı kadar hızlı, gerçek hızı daha yüksek olabilir."),
    ("fnd_video_hw_none_working", "No hardware video encoder works here (missing hardware, driver or ffmpeg support): transcoding runs on the CPU.", "Burada çalışan donanım video kodlayıcı yok (donanım, sürücü ya da ffmpeg desteği eksik): dönüştürme CPU'da yapılır."),
    ("fnd_video_cpu", "CPU encoding (x264): {0} fps, about {1} real-time 1080p30 streams.", "CPU ile kodlama (x264): {0} fps, yaklaşık {1} gerçek zamanlı 1080p30 akış."),
    ("fnd_rt_checksum", "The CPU path-tracing result differs from the reference: this platform calculates differently, so ray-tracing numbers are less comparable.", "CPU path-tracing sonucu referanstan farklı: bu platform farklı hesaplıyor, ışın izleme sayıları daha az karşılaştırılabilir."),
    ("fnd_gpu_not_built", "This build has no GPU support.", "Bu derlemede GPU desteği yok."),
    ("fnd_gpu_error", "GPU {0} could not be tested: {1}", "GPU {0} test edilemedi: {1}"),
    ("fnd_gpu_software", "{0} is a software renderer, not a real GPU: graphics and ray-tracing scores are not given.", "{0} bir yazılım işleyicisi, gerçek GPU değil: grafik ve ışın izleme puanı verilmedi."),
    ("fnd_gpu_tested", "GPU {0} ({1}) tested: {2} GFLOPS FP32.", "GPU {0} ({1}) test edildi: {2} GFLOPS FP32."),
    ("fnd_gpu_matmul_bad", "GPU {0}: the matrix result did not match the CPU reference (driver problem?), its matrix score is ignored.", "GPU {0}: matris sonucu CPU referansıyla uyuşmadı (sürücü sorunu?), matris puanı yok sayıldı."),
    ("fnd_rt_gpu_mismatch", "GPU {0}: the path-traced image brightness differs from the CPU reference, its ray-tracing score is ignored.", "GPU {0}: path-tracing görüntü parlaklığı CPU referansından farklı, ışın izleme puanı yok sayıldı."),
    ("fnd_hw_rt_detected", "GPU {0} reports hardware ray-tracing support (not measured yet; the score uses shader cores).", "GPU {0} donanım ışın izleme desteği bildiriyor (henüz ölçülmedi; puan gölgelendirici çekirdeklerini kullanır)."),
    ("fnd_vram_unknown", "GPU memory size is unknown, so the AI score does not include it. Large language models need enough VRAM.", "GPU bellek miktarı bilinmiyor, bu yüzden yapay zeka puanına dahil edilmedi. Büyük dil modelleri yeterli VRAM ister."),
    ("cmp_cpu_encode", "Video encoding (CPU)", "Video kodlama (CPU)"),
    ("cmp_hw_encode", "Video encoding (hardware)", "Video kodlama (donanım)"),
    ("cmp_rt_cpu", "Ray tracing (CPU)", "Işın izleme (CPU)"),
    ("cmp_gpu_fp32", "GPU compute (FP32)", "GPU hesaplama (FP32)"),
    ("cmp_gpu_mem", "GPU memory bandwidth", "GPU bellek bant genişliği"),
    ("cmp_gpu_matmul", "GPU matrix multiply", "GPU matris çarpımı"),
    ("cmp_gpu_rt", "GPU ray tracing", "GPU ışın izleme"),
    ("cmp_gpu_vram", "GPU memory size", "GPU bellek miktarı"),
    // @@MORE@@
];

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn placeholders(s: &str) -> Vec<usize> {
        let mut v: Vec<usize> = (0..6).filter(|i| s.contains(&format!("{{{i}}}"))).collect();
        v.sort();
        v
    }

    #[test]
    fn table_is_complete_and_consistent() {
        let mut seen = HashSet::new();
        for (k, en, tr) in TABLE {
            assert!(seen.insert(*k), "duplicate key {k}");
            assert!(!en.is_empty() && !tr.is_empty(), "empty text for {k}");
            assert_eq!(placeholders(en), placeholders(tr), "placeholder mismatch in {k}");
        }
    }

    #[test]
    fn every_key_used_in_the_code_is_translated() {
        let sources = [include_str!("render.rs"), include_str!("html.rs"), include_str!("score.rs"), include_str!("main.rs"), include_str!("record.rs"), include_str!("ui.rs"), include_str!("bench/cpu.rs"), include_str!("bench/mem.rs"), include_str!("bench/disk.rs"), include_str!("bench/net.rs")];
        let prefixes = ["prog_", "h_", "l_", "k_", "s_", "cat_", "cmp_", "fc_", "fnd_", "ver_", "msg_", "u_"];
        let keys: HashSet<&str> = TABLE.iter().map(|r| r.0).collect();
        for src in sources {
            let code = src.split("#[cfg(test)]").next().unwrap();
            for lit in code.split('"').skip(1).step_by(2) {
                let is_key = prefixes.iter().any(|p| lit.starts_with(p)) && lit.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_') && lit.len() > 3;
                if is_key && !lit.ends_with('_') {
                    assert!(keys.contains(lit), "missing translation key: {lit}");
                }
            }
        }
    }

    #[test]
    fn turkish_differs_from_english_for_text() {
        let diff = TABLE.iter().filter(|r| r.1 != r.2).count();
        assert!(diff * 10 > TABLE.len() * 8, "most entries should be translated");
    }

    #[test]
    fn substitution_works() {
        assert_eq!(tf(Lang::En, "msg_saved", &["x".to_string()]), "Saved: x");
        assert_eq!(tf(Lang::Tr, "msg_saved", &["x".to_string()]), "Kaydedildi: x");
        assert_eq!(t(Lang::En, "no_such_key"), "no_such_key");
    }

    #[test]
    fn language_parsing() {
        assert_eq!(Lang::parse("1"), Some(Lang::En));
        assert_eq!(Lang::parse(" 2\n"), Some(Lang::Tr));
        assert_eq!(Lang::parse("TR"), Some(Lang::Tr));
        assert_eq!(Lang::parse("xx"), None);
    }
}
