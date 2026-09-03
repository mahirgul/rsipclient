# SIP Hardening Roadmap

SIP alt sisteminde tespit edilen bug'lar ve yapılacak geliştirmeler. Her kalem,
ne yapılacağı ve hangi dosyaların etkileneceği ile birlikte listelenmiştir.
Bu doküman, işe başka bir oturumdan/makineden devam ederken yol haritası olarak
kullanılmak üzere hazırlanmıştır.

## Durum özeti

| Faz | Kapsam | Durum |
|-----|--------|-------|
| 1 | Somut bug'lar (7 kalem) | ✅ Tamamlandı (commit `9a6c38c`) |
| 1b | Auth doğruluğu + servis sertleştirme (küçük kalemler) | ✅ Tamamlandı (v2.5.0) |
| 1c | Config alan doğrulaması + tampon sınırları | ✅ Tamamlandı (v2.5.1) |
| 2 | Mimari (transaction layer, session timers, PRACK) | ⬜ Bekliyor |
| 3 | İleri (NAT traversal, SRTP, SDP parser) | ⬜ Bekliyor |

---

## Faz 1 — Tamamlandı (commit `9a6c38c`)

Aşağıdaki 7 kalem uygulandı ve `cargo fmt` / `cargo clippy -D warnings` /
`cargo test` (36 test) ile doğrulandı.

1. **TLS sertifika doğrulaması** — `Account` → `SipSettings` → `TlsConfig`.
   Yeni config alanları: `tls_verify_cert`, `tls_verify_hostname` (default `true`),
   `tls_ca_cert` (PEM yolu). `transport/tls.rs` artık sertifika/hostname doğruluyor.
2. **`dtmf_mode` dispatch** — `send_dtmf` artık `rfc2833` / `info` / `inband`
   modunu onurlandırıyor (`operations/call.rs`).
3. **Hold/resume doğru RTP portu + re-INVITE auth** — `SipClient.rtp_port` alanı;
   `build_hold_with_auth`; re-INVITE 401/407 challenge işleme.
4. **Digest `qop="auth"`** — `AuthChallenge` struct'ı, `cnonce`/`nc` üretimi,
   `opaque`/`algorithm`/`stale` parse, RFC 2617 §3.5 test vektörü.
5. **Codec saygısı** — INVITE/incoming artık config'deki tek codec'i ilan ediyor
   (`build_sdp_single`); `sdp::parse_remote_codecs` + `warn_codec_mismatch`.
6. **Header doğrulaması** — `validate_header_value` `<` `>` `"` reddi;
   `format_from`/`extra_headers` sanitizasyonu; `config::validate` control-char reddi.
7. **Gelen CANCEL** — kurulum sırasında yarışan CANCEL'e 200 OK + temizlik
   (`service/watcher/incoming_call.rs`).

---

## Faz 1b — Tamamlandı (v2.5.0)

Ayrıştırma/servis katmanındaki somut bug'lar ve "Notlar" başlığındaki küçük
auth kalemleri. `cargo fmt` / `cargo clippy -D warnings` / `cargo test`
(62 test) ile doğrulandı.

1. **Uzaktan tetiklenebilen panic** — `extract_quoted` küçük harfe çevrilmiş
   kopyanın ofsetiyle orijinal satırı dilimliyordu; çok baytlı bir karakter
   (`İ`) ofseti kaydırıp karakter ortasından kesiyordu. Çok baytlı From
   display-name taşıyan bir INVITE, gelen çağrı watcher'ını öldürüyor ve hesap
   yeniden başlatmaya kadar çağrı karşılayamıyordu. Artık tüm başlık/parametre
   eşleşmesi orijinal dize üzerinde ASCII-duyarsız yapılıyor.
2. **Parametre sınırı** — `extract_quoted` anahtarı serbest alt dize olarak
   arıyordu; display-name içindeki `tag=` gerçek parametreyi gölgeleyebiliyordu.
3. **CRLF enjeksiyonu** — çağrı/transfer/mesaj hedefleri ve INFO DTMF hanesi
   doğrulanmadan istek satırına giriyordu (`validate_header_value`,
   `validate_dtmf_digit`; REST + IPC + CLI yollarının tamamı).
4. **`Content-Disposition` panic'i** — ASCII olmayan dosya adı (`kayıt.wav`)
   başlık kurarken `unwrap()` ile çöküyordu; artık RFC 5987 `filename*`.
5. **RTP kaynak filtresi** — simetrik-RTP latch'i; porta erişebilen üçüncü bir
   taraf artık canlı çağrıya ses/DTMF enjekte edemiyor.
6. **RTP başlık ayrıştırma** — CSRC/uzantı/padding hesaba katılıyor (sabit
   12 bayt varsayımı codec'e başlık baytlarını ses diye veriyordu); version≠2
   ve paketi aşan alanlar eleniyor. Kayıt tamponu 30 dk ile sınırlı.
7. **Plugin script dizini** — okuma yolunda da `.rhai/.lua` zorunlu ve
   çözümlenen yol script dizininin altında kalmak zorunda (sembolik bağ kaçışı).
8. **Config sırası** — dashboard artık doğrula → uygula → yaz sırasında
   çalışıyor; başarısız tek hesap diğerlerini düşürmüyor (`failed_accounts`),
   başarısız düzenleme eski istemciyi geri getiriyor.
9. **Sabit zamanlı karşılaştırma** — token/parola karşılaştırmaları erken
   çıkışsız ve kısa devresiz (`secret_eq`).
10. **`Proxy-Authorization`** — 407 challenge'ı artık proxy başlığıyla
    yanıtlanıyor (RFC 3261 §22.3); önceden `Authorization` gönderiliyordu ve
    proxy yeniden challenge ediyordu.
11. **Çoklu challenge** — `extract_auth_challenge` tüm challenge başlıklarını
    tarıyor, status koduna uyanı (407 → proxy) tercih ediyor ve
    yanıtlanamayan bir challenge'ın (nonce'suz Basic) arkasındaki kullanılabilir
    olanı atlamıyor.
12. **`stale=true` yeniden deneme** — REGISTER/UNREGISTER/INVITE, süresi dolmuş
    nonce için taze nonce'la bir kez yeniden deneniyor
    (`stale_retry_challenge`); düz reddedilmeler yeniden denenmiyor.

---

## Faz 1c — Tamamlandı (v2.5.1)

`cargo fmt` / `cargo clippy -D warnings` / `cargo test` (62 test) ile doğrulandı.

1. **Hesap URI alanlarının doğrulanması** — `config::validate` yalnızca
   `display_name`/`asserted_id`/`preferred_id`/`user_agent` alanlarını kontrol
   ediyordu; oysa `username`, `domain` ve `server` istek satırına ve
   From/To/Contact URI'lerine ham olarak giriyor. Hesaplar dashboard'dan
   yazılabildiği için (`POST /api/accounts`, `PUT /api/config`) buradaki bir
   CR/LF hesabın gönderdiği her REGISTER ve INVITE'a başlık enjekte ediyordu.
   Artık control karakter, boşluk ve `<` `>` `"` reddediliyor.
2. **DTMF tampon sınırı** — `take_dtmf_events` hiç çağrılmıyor, `take_dtmf` ise
   yalnızca IVR oturumu tarafından boşaltılıyor; menüsüz auto-answer yapan bir
   hesapta karşı taraf telephone-event gönderdikçe iki tampon da çağrı boyunca
   büyüyordu. v2.5.0'daki kayıt tamponu sınırıyla tutarlı şekilde 512 kalemle
   sınırlandı.

---

## Faz 2 — Mimari

### 1. Transaction layer (RFC 3261 §17) — EN BÜYÜK EKSİK

Mevcut durumda `send()` (`sip/client.rs`) isteği gönderip **gelen ilk paketi**
yanıt olarak kabul eder; branch/CSeq ile eşleştirme yok. Retransmission timer'ları
(Timer A/E/G/F/K) yok. UDP'de gecikmiş/ilgisiz bir paket (eski OPTIONS yanıtı,
başka transaction'ın 1xx'i) yanlış response olarak parse edilir.

Yapılacaklar:

- [ ] İstemci **non-INVITE** transaction FSM (Timer E/F/K): REGISTER, MESSAGE, INFO,
      REFER, SUBSCRIBE.
- [ ] İstemci **INVITE** transaction FSM (Timer A/B): retransmission, provisional
      kabulü, final response eşleme.
- [ ] Sunucu transaction FSM (incoming INVITE/BYE): retransmission filtreleme,
      Timer G/H/I/J.
- [ ] Branch parametresi (`z9hG4bK-...`) ile request→response eşleştirme.
- [ ] `send()`/`recv_timeout` yerine transaction-bazlı alım: gelen mesajın branch +
      CSeq + method ile mevcut transaction'a eşlenmesi.
- [ ] Forking (birden çok final response) temel işleme.

**Etkilenen dosyalar:** `sip/client.rs`, `sip/transport.rs`, yeni
`sip/transaction.rs` (önerilir), `sip/operations/*`.

### 2. Session timers refresh (RFC 4028)

`Session-Expires: 1800;refresher=uac` header'ı zaten `settings.rs`'te üretiliyor
(`Supported: timer` ile birlikte) ama **refresh hiç yapılmıyor** — süre dolunca
oturum sunucu tarafından kesilir.

Yapılacaklar:

- [ ] `Session-Expires` değerini parse et (yanıt + `Min-SE`).
- [ ] Süre dolmadan `UPDATE` veya re-INVITE ile oturumu yenile.
- [ ] `refresher` rolüne göre (uac/uas) kimin yenileyeceğini belirle.
- [ ] Refresh başarısız olursa çağrıyı sonlandır.

### 3. PRACK (RFC 3262 — 100rel)

`Supported: 100rel` ilan ediliyor ama `send_prack` (`sip/client.rs`) `#[allow(dead_code)]`
durumda; reliable provisional response'lar işlenmiyor.

Yapılacaklar:

- [ ] `RSeq` içeren 1xx yanıtını tespit et ve `RAck`'li PRACK gönder.
- [ ] Incoming tarafında PRACK kabul et (yanıt olarak 200 OK).
- [ ] PRACK retransmission / timeout.

---

## Faz 3 — İleri

### 4. NAT traversal (STUN / TURN / ICE)

- [ ] Via header'ına `rport`/`received` parametreleri (symmetric NAT desteği).
- [ ] RFC 5626 outbound (flow token, `reg-id`, `+sip.instance`).
- [ ] ICE: aday toplama, bağlantı kontrolü (STUN binding), TURN relay.

### 5. SRTP (RFC 3711 / 4568)

- [ ] SDP'de `a=crypto` (SDES) teklif/cevap.
- [ ] DTLS-SRTP (`a=fingerprint`, RFC 5763) — modern önerilen yol.
- [ ] RTP gönderim/alım yoluna SRTP şifreleme entegrasyonu.

### 6. SDP parser genişletme

`parse_sdp_connection` (`service/watcher/incoming_call.rs`) sadece ilk `c=` ve
`m=audio`'yu okuyor.

- [ ] Multiple media line (`m=`), port/format seti parse.
- [ ] `a=rtcp`, `a=ice-*`, `a=crypto`, `a=fingerprint` attribute'ları.
- [ ] Per-media `c=` satırı (RFC 4566 §5.7).

### 7. In-band DTMF ton üretimi (Faz 1'den ertelendi)

`dtmf_mode = "inband"` gönderimi şu an `rfc2833`'e düşüyor + uyarı yazıyor.

- [ ] DTMF sinüs tonlarını sentezle (RFC 4733 frekans tablosu).
- [ ] RTP PCM akışına karıştır (`rtp/mod.rs` `send_wav_rtp` benzeri akış).

---

## Notlar / küçük kalemler

- **Tam codec müzakere** — Faz 1'de "config codec'i ilan et + uyar" yapıldı. Dinamik
  müzakere (uzak SDP'ye göre codec seçip RTP gönderim/alım yoluna taşımak)
  `ManagedClient`'taki sabit `codec` alanının yeniden yapılandırılmasını gerektirir.
  Faz 2'de transaction layer ile birlikte ele alınabilir.
- **Çoklu realm denemesi** — challenge seçimi Faz 1b'de düzeltildi, ancak bir
  realm reddettiğinde diğer challenge ile sırayla yeniden deneme hâlâ yok
  (`sip/utils.rs`, `operations/*`). Transaction layer sonrası ele alınması kolay.
- **Incoming CANCEL → 487 (geçerli değil)** — Bu not yanlıştı: auto-answer 200 OK'u
  CANCEL bekleme döngüsünden **önce** gönderiyor (`watcher/incoming_call.rs`), yani
  INVITE'a final yanıt verilmiş oluyor. Bu durumda CANCEL'e 200 OK dönmek doğru;
  487 göndermek ikinci bir final yanıt olur ve RFC'ye aykırıdır. 487, ancak
  auto-answer'dan önce CANCEL yakalanacak şekilde akış değiştirilirse anlamlı olur
  (transaction layer ile birlikte).
- **Hold/transfer için `stale` yeniden deneme** — REGISTER/INVITE'a eklendi;
  `hold_transfer.rs` ve `transfer.rs` yollarında henüz yok.
- **SIP kaynak filtresi (açık)** — `transport/udp.rs` gelen paketin kaynak
  adresini (`_src`) yok sayıyor; SIP portuna erişebilen üçüncü bir taraf sahte
  `BYE` ile canlı çağrıyı düşürebilir veya sahte `INVITE` ile auto-answer
  tetikleyebilir. RTP tarafındaki simetrik latch'in (Faz 1b, kalem 5) SIP
  karşılığı henüz yok. Faz 2.1'deki branch/CSeq eşlemesi bunun büyük kısmını
  kapatır; öncesinde ucuz bir ara çözüm olarak sunucu adresi filtresi eklenebilir.
- **Testler** — Mesaj builder'ları için testler mevcut; parser'a fuzz target ve
  transport/transaction için entegrasyon testleri (SIPp benzeri) eklenmesi değerli.

---

## Önerilen sıra

1. **Transaction layer** (Faz 2.1) — en yüksek öncelik; üstteki yanlış-response
   eşleme, retransmission ve fork sorunlarının çoğunu kökten çözer.
2. **Digest stale/çoklu-realm** (küçük, transaction layer sonrası kolay).
3. **Session timers refresh** + **PRACK** (transaction layer'a dayanır).
4. **NAT traversal** (ICE/STUN/TURN) — ayrı büyük iş.
5. **SRTP** — NAT sonrası.
