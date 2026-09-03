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
| 2 | Mimari (transaction layer, session timers, PRACK) | ✅ Tamamlandı (v2.5.2+) |
| 3 | İleri (rport NAT traversal, SRTP SDES, SDP parser, in-band DTMF) | ✅ Tamamlandı (v2.5.2+) |

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

- [x] İstemci **non-INVITE** transaction FSM (Timer E/F/K): REGISTER, MESSAGE, INFO,
      REFER, SUBSCRIBE (`sip/transaction.rs`).
- [x] İstemci **INVITE** transaction FSM (Timer A/B/D): retransmission, provisional
      kabulü, final response eşleme, otomatik failure ACK (`sip/transaction.rs`).
- [x] Sunucu transaction FSM (incoming INVITE/BYE): retransmission filtreleme,
      mükerrer yanıtlama, otomatik OPTIONS keep-alive responder (`sip/transaction.rs`).
- [x] Branch parametresi (`z9hG4bK-...`) ile request→response eşleştirme (`TransactionKey`).
- [x] `send()`/`recv_timeout` yerine transaction-bazlı alım: gelen mesajın branch +
      CSeq + method ile mevcut transaction'a eşlenmesi (`sip/client.rs`).
- [x] Forking (birden çok final response) ve yanıt demux yönetimi.

**Etkilenen dosyalar:** `sip/client.rs`, `sip/mod.rs`, `sip/transaction.rs`, `sip/operations/*`.

### 2. Session timers refresh (RFC 4028)

`Session-Expires: 1800;refresher=uac` header'ı parse edilip dialog süresince periyodik
olarak yenilenmektedir.

- [x] `Session-Expires` ve `Min-SE` değerini parse et (`sip/utils.rs`).
- [x] Süre dolmadan in-dialog re-INVITE ile oturumu yenile (`refresh_session` in `operations/hold_transfer.rs`).
- [x] Oturum yenileme için arka plan görevi (`spawn_session_refresher` in `managed_client.rs`).
- [x] Refresh başarısız olursa çağrıyı BYE ile sonlandır.

### 3. PRACK (RFC 3262 — 100rel)

Güvenilir geçici yanıtlar (reliable provisional responses) tam desteklenmektedir.

- [x] `RSeq` içeren 1xx yanıtını tespit et ve `RAck`'li PRACK gönder (`execute_invite` in `sip/transaction.rs`).
- [x] Incoming tarafında PRACK kabul et ve 200 OK ile yanıtla (`build_prack_200_ok` in `sip/transaction.rs`).
- [x] PRACK retransmission / timeout FSM.

---

## Faz 3 — İleri

### 4. NAT traversal (STUN / TURN / ICE / RFC 3581 / RFC 5626)

- [x] Tüm Via başlıklarına `rport` parametresi (RFC 3581 symmetric NAT desteği: INVITE, REGISTER, PRACK, BYE, CANCEL, REFER).
- [x] RFC 5626 outbound (`reg-id=1`, `+sip.instance="<urn:uuid:...>"`, `Supported: outbound, path`).
- [ ] ICE: aday toplama, bağlantı kontrolü (STUN binding), TURN relay.

### 5. SRTP (RFC 3711 / 4568)

- [x] SDP'de `a=crypto` (SDES RFC 4568) parse etme ve oluşturma (`SrtpCrypto` in `sip/sdp.rs`).
- [ ] DTLS-SRTP (`a=fingerprint`, RFC 5763).
- [ ] RTP gönderim/alım yoluna SRTP şifreleme entegrasyonu.

### 6. SDP parser genişletme

`parse_sdp` ile RFC 4566, RFC 3605 ve RFC 4568 desteği:

- [x] Media line (`m=audio`), port/format seti ve codec çözümleme.
- [x] `a=rtcp:<port>` (RFC 3605), `a=crypto` (RFC 4568 SDES).
- [x] Medya yönü (`a=sendrecv`, `sendonly`, `recvonly`, `inactive`).
- [x] Per-media `c=` satırı (RFC 4566 §5.7) ve session-level `c=` fallback.

### 7. In-band DTMF ton üretimi

- [x] DTMF sinüs tonlarını sentezle (ITU-T Q.23 / RFC 4733 frekans tablosu: `synthesize_dtmf_pcm`).
- [x] RTP PCM akışına karıştırarak paket gönderimi (`send_dtmf_inband` in `rtp/receiver.rs` ve `send_dtmf` dispatch in `operations/call.rs`).

---

## Notlar / küçük kalemler

- **Tam codec müzakere** — Faz 1'de "config codec'i ilan et + uyar" yapıldı. Dinamik
  müzakere (uzak SDP'ye göre codec seçip RTP gönderim/alım yoluna taşımak)
  `ManagedClient`'taki sabit `codec` alanının yeniden yapılandırılmasını gerektirir.
  Faz 2'de transaction layer ile birlikte ele alınabilir.
- **Çoklu realm denemesi** — ✅ Tamamlandı (`extract_all_auth_challenges`). REGISTER,
  INVITE ve unregister yollarında birden fazla realm sunulursa sırayla denenir (`operations/*`).
- **Hold/transfer için `stale` yeniden deneme ve REFER auth** — ✅ Tamamlandı.
  re-INVITE (hold/resume) ve REFER (transfer) yollarına 401/407 Digest kimlik
  doğrulaması ve `stale=true` taze nonce yeniden denemesi eklendi (`build_refer_with_auth`).
- **SIP kaynak filtresi** — ✅ Tamamlandı. `transport/udp.rs` ve `transport.rs`
  üzerinde `set_peer_filter` desteği eklendi. Gelen UDP paketlerinin kaynak IP adresi
  bilinen `server_addr` / proxy ile eşleşmiyorsa sahte paketler loglanıp düşürülür.
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
