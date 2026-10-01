# Chrome Web Store submission

Everything the dashboard asks for. Paste each block into the field it names.

**Package:** `extension-upload.zip`, from `node scripts/pack-extension.mjs`. It strips the
development `key` from the manifest, which is what Google's guidance says to do, and leaves
out `key.pem`, the icon generator, the notes and any test file. Bump `version` in
`extension/manifest.json` before every upload; Chrome refuses a package whose version has not
increased. This one is **1.0.3**.

**Images:** `store-assets/`, from `node scripts/build-store-assets.mjs` (rerun it whenever the
popup or the mark changes). They are photographs of the real popup, rendered with its own HTML, CSS and
JavaScript — not mock-ups.

**Language.** The dashboard localizes the **listing** — name, summary, description, images —
once you add a language under Store listing, so both are given below. The **privacy tab** is
one value per field rather than one per language; nothing obliges those to be English, so they
are given in Turkish, which is the language of this account's dashboard. The English wording is
kept in the appendix for a reviewer exchange that happens in English.

**Before uploading 1.0.3, know two things.**

- **Existing users will see the extension switched off.** 1.0.3 asks for more than 1.0.2 did
  (`webRequest`, `scripting`, access to all sites). Chrome disables an installed extension whose
  update adds permissions until the user accepts them, with a prompt in the toolbar menu.
- **YouTube is a policy risk.** The Chrome Web Store has refused and removed extensions that
  download from YouTube. The popup offers YouTube pages because that was chosen knowing the
  risk. If review objects, put YouTube's three hosts (`youtube.com`, `youtu.be`,
  `music.youtube.com`) into `UNLISTED_SITES` in `extension/media.js`: the popup then lists
  nothing on YouTube pages or for YouTube players embedded elsewhere. Taking YouTube out of
  `PAGE_SITES` alone is not enough, because the page fallback offers any page with a video
  element on it.

---

## 1. Store listing — English

**Item name**

```
Universal Downloader Connector
```

**Summary** (132 characters at most)

```
Sends the videos playing in your browser to the Universal Downloader app on this computer, one button per video.
```

**Description**

```
Universal Downloader Connector is the browser half of Universal Downloader, a free desktop application for downloading media.

Play a video on any site and click the extension's icon. The popup lists what the page is playing — streams, video files, audio — each with a Get button. Press it and Universal Downloader comes to the front and downloads it at your default quality. The extension never downloads anything itself.

On YouTube, Vimeo, X, Instagram, TikTok and the other sites the app knows, the popup offers the page itself, so the app gets the right title and every quality.

For members-only YouTube videos you already pay for, turn on "YouTube session" in the popup. It lends your YouTube sign-in to the app on this computer. It is off until you turn it on.

WHAT IT DOES NOT DO

- It has no server, no analytics and no account. Nothing it sees goes to the developer or to any third party.
- Nothing leaves your computer. The only recipient is the Universal Downloader application, which you installed, and a video goes to it only when you press Get.
- It does not get around DRM. Protected videos are marked "Protected" and have no button, and services such as Netflix are listed as protected.

HOW IT WORKS

1. Install Universal Downloader on your computer and open it once.
2. Add this extension. A page opens explaining the rest.
3. Play a video, click the toolbar icon, press Get.

Universal Downloader is open source: https://github.com/unsalable/downloader
```

**Category:** Tools · **Language:** English

**Images:** `store-assets/en/1-videos.png`, `2-sites.png`, `3-privacy.png` (1280×800),
small promo tile `store-assets/en/promo-tile-440x280.png` (440×280).

---

## 2. Store listing — Türkçe

Mağaza girişi bölümünden dil seçiciyle Türkçe'yi ekleyip bunları doldurun.

**Öğe adı**

```
Universal Downloader Connector
```

**Özet** (en çok 132 karakter)

```
Tarayıcınızda oynayan videoları, her biri için tek düğmeyle bu bilgisayardaki Universal Downloader uygulamasına gönderir.
```

**Açıklama**

```
Universal Downloader Connector, ücretsiz bir masaüstü indirme uygulaması olan Universal Downloader'ın tarayıcı tarafıdır.

Herhangi bir sitede bir video oynatın ve eklentinin simgesine tıklayın. Açılır pencere, sayfanın oynattıklarını — akışları, video dosyalarını, sesleri — her birinin yanında bir İndir düğmesiyle listeler. Bastığınızda Universal Downloader öne gelir ve onu varsayılan kalitenizle indirir. Eklentinin kendisi hiçbir şey indirmez.

YouTube, Vimeo, X, Instagram, TikTok ve uygulamanın tanıdığı diğer sitelerde açılır pencere sayfanın kendisini sunar; böylece uygulama doğru başlığı ve her kaliteyi alır.

Zaten ödediğiniz, üyelere özel YouTube videoları için açılır penceredeki "YouTube oturumu" anahtarını açın. YouTube oturumunuzu bu bilgisayardaki uygulamaya verir. Siz açana kadar kapalıdır.

YAPMADIKLARI

- Sunucusu, analitiği ve hesabı yoktur. Gördüğü hiçbir şey geliştiriciye veya üçüncü bir tarafa gitmez.
- Hiçbir şey bilgisayarınızdan çıkmaz. Tek alıcı, sizin kurduğunuz Universal Downloader uygulamasıdır ve bir video ona yalnızca İndir'e bastığınızda gider.
- DRM'yi aşmaz. Korumalı videolar "Korumalı" olarak işaretlenir ve düğmeleri olmaz; Netflix gibi servisler korumalı olarak gösterilir.

NASIL ÇALIŞIR

1. Universal Downloader'ı bilgisayarınıza kurun ve bir kez açın.
2. Bu eklentiyi ekleyin. Gerisini anlatan bir sayfa kendiliğinden açılır.
3. Bir video oynatın, araç çubuğundaki simgeye tıklayın, İndir'e basın.

Universal Downloader açık kaynaktır: https://github.com/unsalable/downloader
```

**Kategori:** Tools · **Dil:** Türkçe

**Görseller:** `store-assets/tr/1-videos.png`, `2-sites.png`, `3-privacy.png` (1280×800),
küçük tanıtım karosu `store-assets/tr/promo-tile-440x280.png` (440×280).

---

## 3. Gizlilik uygulamaları — alan başına tek değer

Bunlar dile göre çoğalmaz: her alan tek bir değer alır ve onu inceleyen bir kişi okur. Her
izin `extension/manifest.json`'daki bir girdiye karşılık gelir; manifest başka bir şey istemez.

**Tek amaç**

```
Kullanıcının tarayıcısında oynayan videoları, aynı bilgisayardaki ve kullanıcının kendisinin kurduğu Universal Downloader masaüstü uygulamasına göndermek; böylece uygulama onları indirebilir. Kullanıcı açarsa, uygulamanın kullanıcının zaten izleyebildiği üyelere özel YouTube videolarını da indirebilmesi için YouTube oturumu da aynı uygulamaya verilir. Eklentinin kendisi hiçbir şey indirmez.
```

**İzin: webRequest**

```
Bir sekmenin hangi videoları ve sesleri oynattığını görmek için. Eklenti, sekmelerin medya, XHR ve fetch yanıtlarının başlıklarına bakar (adres, içerik türü, boyut) ve video ya da ses taşıyanları — HLS ve DASH akışlarını, video ve ses dosyalarını — o sekmenin listesine ekler. İstekleri engellemez, değiştirmez veya yönlendirmez; yalnızca yanıt başlıklarını okur. Liste tarayıcının oturum deposunda tutulur, sekme başka bir sayfaya geçince veya kapanınca silinir ve kullanıcı İndir'e basmadıkça hiçbir yere gönderilmez.
```

**İzin: scripting**

```
Kullanıcı araç çubuğundaki simgeye tıkladığında, yalnızca o anki sekmede iki küçük işlev çalıştırmak için: biri sayfanın başlığını, önizleme görselini ve <video>/<audio> öğelerini (adres, kapak, süre, çözünürlük, DRM kullanılıp kullanılmadığı) okur; diğeri bir akışın oynatma listesini, onu ilk yükleyen çerçevenin içinden yeniden okuyarak kalitesini ve süresini öğrenir. Bu işlevler sayfayı değiştirmez, kalıcı bir betik bırakmaz ve kullanıcı tıklamadan hiç çalışmaz.
```

**Ana makine izni: <all_urls>**

```
Videolar, sitenin kullandığı her türlü sunucudan gelir (CDN'ler, oynatıcı alan adları), bu yüzden webRequest ile hangi videoların oynadığını görmek ve kullanıcının tıkladığı sekmede scripting kullanmak için tüm sitelere erişim gerekir; daha dar bir kalıp, kullanıcının izlediği videoların çoğunu kaçırır. Bu erişim yalnızca medyayı tanımak ve listelemek için kullanılır. Çerezler yalnızca youtube.com için ve yalnızca kullanıcı YouTube oturumu anahtarını açtığında okunur.
```

**İzin: cookies**

```
Yalnızca youtube.com çerezleri ve yalnızca kullanıcı açılır penceredeki "YouTube oturumu" anahtarını açtığında. Çerezler, kullanıcının aynı bilgisayara kurduğu Universal Downloader uygulamasına aktarılır; böylece uygulamanın indirme motoru oturum açmış kullanıcı olarak davranabilir ve kullanıcının parasını ödediği, yalnızca üyelere açık videoları indirebilir. Chrome 127 ve sonrası çerez veritabanını aynı makinedeki diğer programlara karşı şifrelediği için uygulama onu artık doğrudan okuyamıyor; yerel mesajlaşmayla birlikte chrome.cookies bunun desteklenen yoludur. Başka hiçbir sitenin çerezi okunmaz.
```

**İzin: nativeMessaging**

```
Bu eklentinin iletişim kurduğu tek kanaldır. Kullanıcının İndir'e bastığı videonun adresini ve — anahtar açıkken — YouTube oturumunu, Universal Downloader masaüstü uygulamasının kurduğu com.universaldownloader.bridge adlı yerel mesajlaşma sunucusuna Chrome'un stdio kanalı üzerinden gönderir. O sunucuyu Chrome'un kendisi başlatır ve yalnızca bu eklentinin kimliği için başlatır. Hiçbir ağ portu dinlenmez; eklentinin bir sunucusu yoktur.
```

**İzin: storage**

```
chrome.storage.local içinde üç küçük değer: bu tarayıcı profili için rastgele bir tanımlayıcı (masaüstü uygulaması bir Chrome profilini diğerinden ayırabilsin diye), kullanıcının YouTube oturumunu kapatıp kapatmadığı ve uygulamanın bu profilin bağlantısı hakkında en son ne dediği (bağlı, uygulamada kapalı ya da bağlı değil). chrome.storage.session içinde, bellekte, her sekmenin oynattığı medyanın listesi; sekme kapanınca ve tarayıcı kapanınca silinir. Çerez içeriği saklanmaz.
```

**İzin: alarms**

```
Günde bir kez çalışan tek bir alarm, YouTube oturumu açıksa mevcut oturumu yeniden göndererek masaüstü uygulamasının elindeki kopyanın eskimesini önler. Anahtar kapalıysa alarm çerez göndermez.
```

**Uzaktan kod:** Hayır, uzaktan kod kullanmıyorum.

```
Tüm mantık yüklenen paketin içindedir. Eklenti hiçbir yerden betik yüklemez, çalışma zamanında hiçbir kod değerlendirmez ve küçültülmemiş ya da gizlenmemiştir; background.js ve media.js yayınlandığı haliyle okunabilir. scripting ile çalıştırılan iki işlev de pakettedir.
```

**Veri kullanımı**

Şu dört kutuyu işaretleyin. Hiçbiri geliştiriciye gitmez, ama hepsi eklentinin elinden geçer ve
kullanıcı İndir'e bastığında ya da anahtarı açtığında aynı bilgisayardaki uygulamaya aktarılır;
işaretlememek, açıklamayla çelişen bir beyan olur:

- **Kimlik doğrulama bilgileri** — youtube.com çerezleri, yalnızca anahtar açıkken.
- **Web geçmişi** — İndir'e basılan sekmenin adresi ve başlığı.
- **Kullanıcı etkinliği** — ağ izleme: sekmelerin medya isteklerine bakılır.
- **Web sitesi içeriği** — sayfanın başlığı, önizleme görseli ve medya adresleri.

Diğerlerini (kişisel kimlik, sağlık, finans, kişisel iletişim, konum) işaretlemeyin; eklenti
onlara dokunmaz.

Verinin ne için kullanıldığını soran kutuya:

```
Sekmelerin oynattığı medyanın adresleri, eklentinin kendi açılır penceresinde listelenmek için tarayıcının oturum deposunda tutulur ve sekme kapanınca silinir. Kullanıcı İndir'e bastığında yalnızca o öğenin adresi ve türü, sayfanın adresi ve başlığı, isteğin yönlendiren adresi ve kökeni, tarayıcının kullanıcı aracısı dizesi ve önizleme görselinin adresi, Chrome'un yerel mesajlaşma kanalı üzerinden kullanıcının aynı bilgisayara kendisinin kurduğu masaüstü uygulamasına aktarılır. youtube.com kimlik doğrulama çerezleri yalnızca kullanıcı YouTube oturumu anahtarını açtığında aynı yolla aynı uygulamaya aktarılır. Hiçbiri geliştirici tarafından toplanmaz, hiçbir sunucuya iletilmez ve kimseyle paylaşılmaz.
```

Üç beyanı da onaylayın; burada üçü de doğrudur:

- verinin üçüncü taraflara satılmadığı;
- verinin, beyan edilen tek amaçla ilgisi olmayan bir amaç için kullanılmadığı veya
  aktarılmadığı;
- verinin kredi değerlendirmesi amacıyla kullanılmadığı veya aktarılmadığı.

**Gizlilik politikası URL'si**

```
https://github.com/unsalable/downloader/blob/main/extension/PRIVACY.md
```

`extension/PRIVACY.md` bunun için yazıldı ve iki dili de taşıyor. **Göndermeden önce içine bir
iletişim adresi koyun** — her iki bölümde de yerini gösteren bir yorum satırı var ve mağaza bu
adresi kullanıcılara gösterir.

---

## 4. Mağaza kimliği

Mağaza kendi eklenti kimliğini üretir ve geliştirme `key` alanını yok sayar; yayımlanan eklentinin
kimliği `oikcjjcihkfmgmmmjagilnfgnfilghic`, paketsiz kurulanınki `bkoicficlaelgjpjhlddhloepoocpfoj`.
Masaüstü uygulaması tanımadığı hiçbir kimliğe cevap vermez, bu yüzden ikisi de
`src-tauri/src/bridge/protocol.rs` içinde durur (`EXTENSION_ID_STORE`, `EXTENSION_ID_DEV`).
Mağaza kimliğini tanımayan bir uygulama sürümü, mağazadan kurulan eklentide "Universal
Downloader'ı güncelleyin" olarak görünür; o sürüm yayımlanmadan bu eklentiyi duyurmayın.

`extension/key.pem` dosyasını yeniden üretmeyin. Geliştirme kimliğini o sabitler; yenisi,
`EXTENSION_ID_DEV` güncellenene kadar host'un geliştirme derlemelerine cevap vermesini sessizce
durdurur.

---

## Ek — the same privacy answers in English

Kept for a review exchange that happens in English, or if the dashboard language changes.
These are the same statements as section 3, not different ones.

**Single purpose**

```
Sending the videos playing in the user's browser to the Universal Downloader desktop application, which the user installed on the same computer, so that it can download them. If the user turns it on, the user's YouTube session is lent to the same application as well, so it can download members-only YouTube videos the user can already watch. The extension itself downloads nothing.
```

**Permission: webRequest**

```
To see which videos and sounds a tab plays. The extension looks at the response headers of tabs' media, XHR and fetch requests (address, content type, size) and adds the ones carrying video or audio — HLS and DASH streams, video and audio files — to that tab's list. It does not block, modify or redirect any request; it only reads response headers. The list is kept in the browser's session storage, dropped when the tab moves to another page or closes, and sent nowhere unless the user presses Get.
```

**Permission: scripting**

```
When the user clicks the toolbar icon, and only in that tab, to run two small functions: one reads the page's title, preview image and <video>/<audio> elements (address, poster, length, resolution, whether DRM is in use); the other reads a stream's playlist again from inside the frame that first loaded it, to learn its quality and length. Neither changes the page, neither leaves a script behind, and neither runs without the user's click.
```

**Host permission: <all_urls>**

```
Videos come from whatever servers a site uses (CDNs, player domains), so seeing which videos play with webRequest, and using scripting in the tab the user clicked on, needs access to all sites; any narrower pattern misses most of what users watch. The access is used only to recognise and list media. Cookies are read for youtube.com only, and only when the user turns the YouTube session switch on.
```

**Permission: cookies**

```
youtube.com cookies only, and only when the user turns on the "YouTube session" switch in the popup. They are passed to Universal Downloader, which the user installed on the same computer, so its download engine can act as the signed-in user and download members-only videos the user pays for. Chrome 127 and later encrypt the cookie database against other local programs, so the application can no longer read it directly; chrome.cookies with native messaging is the supported path. No other site's cookies are read.
```

**Permission: nativeMessaging**

```
The only channel this extension communicates over. It sends the address of the video the user pressed Get on and — while the switch is on — the YouTube session to com.universaldownloader.bridge, a native messaging host installed by the Universal Downloader desktop application, using Chrome's stdio channel. Chrome starts that host itself and only for this extension's ID. Nothing listens on a network port; the extension has no server.
```

**Permission: storage**

```
Three small values in chrome.storage.local: a random identifier for this browser profile (so the desktop application can tell one Chrome profile from another), whether the user turned the YouTube session off, and what the application last said about this profile's connection (connected, turned off in the application, or not connected). In chrome.storage.session, in memory, each tab's list of the media it played, dropped when the tab or the browser closes. No cookie content is stored.
```

**Permission: alarms**

```
One alarm, once a day, which re-sends the current session while the YouTube session switch is on, so the copy held by the desktop application does not expire. With the switch off, the alarm sends no cookies.
```

**Remote code:** No, I am not using remote code.

```
All logic is contained in the uploaded package. The extension loads no scripts from anywhere, evaluates nothing at runtime, and is not minified or obfuscated — background.js and media.js can be read as shipped. Both functions run with scripting are in the package.
```

**Data usage**

Tick Authentication information, Web history, User activity and Website content.

```
The addresses of the media tabs play are kept in the browser's session storage, to be listed in the extension's own popup, and dropped when the tab closes. When the user presses Get, only that item's address and kind, the page's address and title, the request's referrer and origin, the browser's user-agent string and the preview image's address are passed, over Chrome's native messaging channel, to a desktop application the user installed on the same computer. youtube.com authentication cookies are passed the same way to the same application only when the user turns the YouTube session switch on. None of it is collected by the developer, transmitted to any server or shared with anyone.
```
