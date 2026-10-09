# Chrome Web Store submission

Everything the dashboard asks for. Paste each block into the field it names.

**Package:** `extension-upload.zip`, from `node scripts/pack-extension.mjs`. It strips the
development `key` from the manifest, which is what Google's guidance says to do, and leaves
out `key.pem`, the icon generator, the notes and any test file. Bump `version` in
`extension/manifest.json` before every upload; Chrome refuses a package whose version has not
increased. This one is **1.0.5**.

**Images:** `store-assets/`, from `node scripts/build-store-assets.mjs` (rerun it whenever the
popup or the mark changes). They are photographs of the real popup, rendered with its own HTML, CSS and
JavaScript — not mock-ups.

**Language.** The dashboard localizes the **listing** — name, summary, description, images —
once you add a language under Store listing, so both are given below. The **privacy tab** is
one value per field rather than one per language; nothing obliges those to be English, so they
are given in Turkish, which is the language of this account's dashboard. The English wording is
kept in the appendix for a reviewer exchange that happens in English.

**Before uploading 1.0.5, know three things.**

- **No new permissions.** 1.0.5 asks for exactly what 1.0.4 did, so Chrome updates it without
  switching it off. Only someone still on 1.0.2 sees it disabled until they accept what 1.0.3
  added (`webRequest`, `scripting`, access to all sites).
- **It reads a second site's cookies.** The new "TikTok session" switch reads tiktok.com cookies
  while it is on, where every 1.0.4 answer said "youtube.com only". Each answer below says so
  now: paste them all again, the privacy tab included, and publish the updated
  `extension/PRIVACY.md` before the package goes in. If review asks why, the answer is the single
  purpose — age-restricted posts the user's own account can already see, read only while the
  switch is on, passed only to the app on the same computer.
- **And, on Get, the site the user is on.** A third switch, "Other sites", off until turned on,
  reads one site's cookies — the registrable domain of the page Get is pressed on, never YouTube's
  or TikTok's — at that press and at no other time, and the app keeps them sealed for at most an
  hour. This is the answer review is likeliest to question, because it is not one named site:
  the answers below say plainly that it is the page's own site, only on the user's press, only to
  the app on the same computer.
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

Play a video on any site and click the extension's icon. The popup lists what the page is playing — streams, video files, audio — each with a Get button. Press it and Universal Downloader comes to the front with the video open, so you can pick the quality and the watermark before it downloads. The extension never downloads anything itself.

On YouTube, Vimeo, X, Instagram, TikTok and the other sites the app knows, the popup offers the page itself, so the app gets the right title and every quality.

For members-only YouTube videos you already pay for, turn on "YouTube session" in the popup; for age-restricted TikTok posts your own account can see, "TikTok session", which the popup shows on TikTok; for posts on any other site that only your account can see, "Other sites", which lends the sign-in of the site you press Get on, for that download. Each lends only to the app on this computer, and is off until you turn it on.

WHAT IT DOES NOT DO

- It has no server, no analytics and no account. Nothing it sees goes to the developer or to any third party.
- Nothing leaves your computer. The only recipient is the Universal Downloader application, which you installed. It says what each video would download as before you press anything, and downloads one only when you press Get.
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

Herhangi bir sitede bir video oynatın ve eklentinin simgesine tıklayın. Açılır pencere, sayfanın oynattıklarını — akışları, video dosyalarını, sesleri — her birinin yanında bir İndir düğmesiyle listeler. Bastığınızda Universal Downloader video açık olarak öne gelir; indirmeden önce kaliteyi ve filigranı seçersiniz. Eklentinin kendisi hiçbir şey indirmez.

YouTube, Vimeo, X, Instagram, TikTok ve uygulamanın tanıdığı diğer sitelerde açılır pencere sayfanın kendisini sunar; böylece uygulama doğru başlığı ve her kaliteyi alır.

Zaten ödediğiniz, üyelere özel YouTube videoları için açılır penceredeki "YouTube oturumu" anahtarını; kendi hesabınızın görebildiği yaş sınırlı TikTok gönderileri için TikTok'tayken görünen "TikTok oturumu" anahtarını; başka bir sitede yalnızca hesabınızın görebildiği gönderiler için de "Diğer siteler" anahtarını açın. Diğer siteler, İndir'e bastığınız sitenin oturumunu yalnızca o indirme için verir. Her anahtar oturumu yalnızca bu bilgisayardaki uygulamaya verir ve siz açana kadar kapalıdır.

YAPMADIKLARI

- Sunucusu, analitiği ve hesabı yoktur. Gördüğü hiçbir şey geliştiriciye veya üçüncü bir tarafa gitmez.
- Hiçbir şey bilgisayarınızdan çıkmaz. Tek alıcı, sizin kurduğunuz Universal Downloader uygulamasıdır. Uygulama her videonun nasıl ineceğini siz bir şeye basmadan söyler, ama yalnızca siz uygulamada indirmeyi seçtiğinizde indirir.
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
Kullanıcının tarayıcısında oynayan videoları, aynı bilgisayardaki ve kullanıcının kendisinin kurduğu Universal Downloader masaüstü uygulamasına göndermek; böylece uygulama onları indirebilir. Kullanıcı açarsa, uygulamanın kullanıcının zaten izleyebildiği üyelere özel YouTube videolarını ve kullanıcının kendi TikTok hesabının görebildiği yaş sınırlı TikTok gönderilerini de indirebilmesi için o sitenin oturumu da aynı uygulamaya verilir; kullanıcı "Diğer siteler" anahtarını açarsa, başka bir sitede İndir'e bastığında o sitenin oturumu da yalnızca o indirme için verilir. Her anahtar ayrıdır. Eklentinin kendisi hiçbir şey indirmez.
```

**İzin: webRequest**

```
Bir sekmenin hangi videoları ve sesleri oynattığını görmek için. Eklenti, sekmelerin medya, XHR ve fetch yanıtlarının başlıklarına bakar (adres, içerik türü, boyut) ve video ya da ses taşıyanları — HLS ve DASH akışlarını, video ve ses dosyalarını — o sekmenin listesine ekler. İstekleri engellemez, değiştirmez veya yönlendirmez; yalnızca yanıt başlıklarını okur. Liste tarayıcının oturum deposunda tutulur, sekme başka bir sayfaya geçince veya kapanınca silinir ve aynı bilgisayardaki Universal Downloader uygulamasından başka hiçbir yere gönderilmez: açılır pencere açıkken, her birinin nasıl ineceğini söylemesi için listelenen ilk birkaç öğe; kullanıcı İndir'e bastığında da indirmesi için o öğe.
```

**İzin: scripting**

```
Kullanıcı araç çubuğundaki simgeye tıkladığında, yalnızca o anki sekmede iki küçük işlev çalıştırmak için: biri sayfanın başlığını, önizleme görselini ve <video>/<audio> öğelerini (adres, kapak, süre, çözünürlük, DRM kullanılıp kullanılmadığı) okur; diğeri bir akışın oynatma listesini, onu ilk yükleyen çerçevenin içinden yeniden okuyarak kalitesini ve süresini öğrenir. Bu işlevler sayfayı değiştirmez, kalıcı bir betik bırakmaz ve kullanıcı tıklamadan hiç çalışmaz.
```

**Ana makine izni: <all_urls>**

```
Videolar, sitenin kullandığı her türlü sunucudan gelir (CDN'ler, oynatıcı alan adları), bu yüzden webRequest ile hangi videoların oynadığını görmek ve kullanıcının tıkladığı sekmede scripting kullanmak için tüm sitelere erişim gerekir; daha dar bir kalıp, kullanıcının izlediği videoların çoğunu kaçırır. Bu erişim yalnızca medyayı tanımak ve listelemek için kullanılır. Çerezler youtube.com ve tiktok.com için, her biri yalnızca kullanıcı o sitenin oturum anahtarını (YouTube oturumu, TikTok oturumu) açtığında okunur; başka bir sitenin çerezleri yalnızca "Diğer siteler" anahtarı açıkken ve yalnızca kullanıcı o sitenin sayfasında İndir'e bastığı anda, yalnızca o sitenin alan adı için okunur.
```

**İzin: cookies**

```
youtube.com ve tiktok.com çerezleri, her biri yalnızca kullanıcı açılır penceredeki o sitenin anahtarını ("YouTube oturumu", "TikTok oturumu") açtığında ve anahtar açık kaldığı sürece; "Diğer siteler" anahtarı açıkken de, kullanıcının İndir'e bastığı sayfanın sitesinin çerezleri, yalnızca o basışta ve yalnızca o sitenin alan adı için (YouTube ve TikTok hariç). Çerezler, kullanıcının aynı bilgisayara kurduğu Universal Downloader uygulamasına aktarılır; böylece uygulamanın indirme motoru oturum açmış kullanıcı olarak davranabilir ve kullanıcının parasını ödediği, yalnızca üyelere açık YouTube videolarını ve kullanıcının kendi TikTok hesabının görebildiği yaş sınırlı TikTok gönderilerini, başka bir sitede de kullanıcının kendi hesabının görebildiği gönderiyi indirebilir; uygulama diğer sitenin çerezlerini en fazla bir saat şifreli tutar. Chrome 127 ve sonrası çerez veritabanını aynı makinedeki diğer programlara karşı şifrelediği için uygulama onu artık doğrudan okuyamıyor; yerel mesajlaşmayla birlikte chrome.cookies bunun desteklenen yoludur. Bunların dışında hiçbir çerez okunmaz ve arka planda hiçbir zaman başka bir sitenin çerezi okunmaz.
```

**İzin: nativeMessaging**

```
Bu eklentinin iletişim kurduğu tek kanaldır. Açılır pencerede listelenen ilk birkaç videonun adresini (uygulama her birinin hangi kalitede, hangi biçimde ve yaklaşık ne boyutta ineceğini söylesin diye), kullanıcının İndir'e bastığı videonun adresini ve — anahtarları açıkken — YouTube ve TikTok oturumlarını ve İndir'e basılan sayfanın sitesinin oturumunu, Universal Downloader masaüstü uygulamasının kurduğu com.universaldownloader.bridge adlı yerel mesajlaşma sunucusuna Chrome'un stdio kanalı üzerinden gönderir. O sunucuyu Chrome'un kendisi başlatır ve yalnızca bu eklentinin kimliği için başlatır. Hiçbir ağ portu dinlenmez; eklentinin bir sunucusu yoktur.
```

**İzin: storage**

```
chrome.storage.local içinde beş küçük değer: bu tarayıcı profili için rastgele bir tanımlayıcı (masaüstü uygulaması bir Chrome profilini diğerinden ayırabilsin diye), kullanıcının YouTube oturumunu kapatıp kapatmadığı, TikTok oturumunu açıp açmadığı, Diğer siteler anahtarını açıp açmadığı ve uygulamanın bu profilin bağlantısı hakkında en son ne dediği (bağlı, uygulamada kapalı ya da bağlı değil). chrome.storage.session içinde, bellekte, her sekmenin oynattığı medyanın listesi; sekme kapanınca ve tarayıcı kapanınca silinir. Çerez içeriği saklanmaz.
```

**İzin: alarms**

```
Günde bir kez çalışan tek bir alarm, YouTube ya da TikTok oturumu açıksa o sitenin oturumunu yeniden göndererek masaüstü uygulamasının elindeki kopyanın eskimesini önler. İki anahtar da kapalıysa alarm çerez göndermez; Diğer siteler anahtarı açık olsa da alarm hiçbir zaman başka bir sitenin çerezini göndermez.
```

**Uzaktan kod:** Hayır, uzaktan kod kullanmıyorum.

```
Tüm mantık yüklenen paketin içindedir. Eklenti hiçbir yerden betik yüklemez, çalışma zamanında hiçbir kod değerlendirmez ve küçültülmemiş ya da gizlenmemiştir; background.js ve media.js yayınlandığı haliyle okunabilir. scripting ile çalıştırılan iki işlev de pakettedir.
```

**Veri kullanımı**

Şu dört kutuyu işaretleyin. Hiçbiri geliştiriciye gitmez, ama hepsi eklentinin elinden geçer ve
kullanıcı İndir'e bastığında ya da bir oturum anahtarını açtığında aynı bilgisayardaki uygulamaya aktarılır;
işaretlememek, açıklamayla çelişen bir beyan olur:

- **Kimlik doğrulama bilgileri** — youtube.com ve tiktok.com çerezleri, her biri yalnızca kendi anahtarı açıkken; Diğer siteler açıkken İndir'e basılan sayfanın sitesinin çerezleri, yalnızca o basışta.
- **Web geçmişi** — İndir'e basılan sekmenin adresi ve başlığı.
- **Kullanıcı etkinliği** — ağ izleme: sekmelerin medya isteklerine bakılır.
- **Web sitesi içeriği** — sayfanın başlığı, önizleme görseli ve medya adresleri.

Diğerlerini (kişisel kimlik, sağlık, finans, kişisel iletişim, konum) işaretlemeyin; eklenti
onlara dokunmaz.

Verinin ne için kullanıldığını soran kutuya:

```
Sekmelerin oynattığı medyanın adresleri, eklentinin kendi açılır penceresinde listelenmek için tarayıcının oturum deposunda tutulur ve sekme kapanınca silinir. Açılır pencere açıkken, listelenen ilk birkaç öğe için aşağıdaki bilgiler, uygulama her birinin nasıl ineceğini söyleyebilsin diye aynı uygulamaya sorulur; uygulama bunun için hiçbir şey indirmez ve saklamaz. Kullanıcı İndir'e bastığında yalnızca o öğenin adresi ve türü, sayfanın adresi ve başlığı, isteğin yönlendiren adresi ve kökeni, tarayıcının kullanıcı aracısı dizesi ve önizleme görselinin adresi, Chrome'un yerel mesajlaşma kanalı üzerinden kullanıcının aynı bilgisayara kendisinin kurduğu masaüstü uygulamasına aktarılır. youtube.com ve tiktok.com kimlik doğrulama çerezleri, her biri yalnızca kullanıcı o sitenin oturum anahtarını açtığında, aynı yolla aynı uygulamaya aktarılır; kullanıcı Diğer siteler anahtarını açtıysa, İndir'e bastığı sayfanın sitesinin çerezleri de yalnızca o basışta aynı yolla aktarılır ve uygulama onları en fazla bir saat tutar. Hiçbiri geliştirici tarafından toplanmaz, hiçbir sunucuya iletilmez ve kimseyle paylaşılmaz.
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

## 5. addons.mozilla.org

**Package:** `extension-firefox.zip`, from `node scripts/pack-extension.mjs --firefox`: the same files
and the same version as Chrome's, with only the manifest changed (see `extension/README.md`). The
listing text in sections 1 and 2 fits AMO's fields as it is.

**Data collection.** The Firefox manifest declares `data_collection_permissions: { required:
["none"] }`, and 1.0.5 keeps it: nothing the extension handles leaves the computer or reaches the
developer. The cookies the session switches read go to the Universal Downloader application on
the same computer over native messaging, as YouTube's already did in 1.0.4. If a reviewer counts
that as collecting authentication information all the same, the category that fits is
`authenticationInfo`, as optional rather than required, since nothing is read until the user turns
a switch on. The declaration lives in `forFirefox()` in `scripts/pack-extension.mjs`.

**Notes to reviewer**

```
The extension has no server and sends nothing over the network. It talks only to com.universaldownloader.bridge, a native messaging host installed by the Universal Downloader desktop application on the same computer. Three switches in the popup, "YouTube session", "TikTok session" and "Other sites", are off until the user turns them on. The first two each let the extension read that one site's cookies (youtube.com or tiktok.com) and pass them to that application, so it can download members-only YouTube videos the user pays for and age-restricted TikTok posts the user's own account can see. "Other sites" reads the cookies of one site only when the user presses Get on that site's page -- the page's registrable domain, never YouTube's or TikTok's -- and the application keeps them, encrypted, for at most an hour. Turning a switch off tells the application to delete what it holds. No source file is minified or generated.
```

---

## Ek — the same privacy answers in English

Kept for a review exchange that happens in English, or if the dashboard language changes.
These are the same statements as section 3, not different ones.

**Single purpose**

```
Sending the videos playing in the user's browser to the Universal Downloader desktop application, which the user installed on the same computer, so that it can download them. If the user turns them on, the user's YouTube or TikTok session is lent to the same application as well, so it can download members-only YouTube videos the user can already watch and age-restricted TikTok posts the user's own account can see; with "Other sites" on, pressing Get on another site's page lends that site's session for that download. Each switch is separate. The extension itself downloads nothing.
```

**Permission: webRequest**

```
To see which videos and sounds a tab plays. The extension looks at the response headers of tabs' media, XHR and fetch requests (address, content type, size) and adds the ones carrying video or audio — HLS and DASH streams, video and audio files — to that tab's list. It does not block, modify or redirect any request; it only reads response headers. The list is kept in the browser's session storage, dropped when the tab moves to another page or closes, and passed nowhere but to the Universal Downloader application on the same computer: the first few listed items while the popup is open, so the application can say how each would download, and an item the user presses Get on, to download it.
```

**Permission: scripting**

```
When the user clicks the toolbar icon, and only in that tab, to run two small functions: one reads the page's title, preview image and <video>/<audio> elements (address, poster, length, resolution, whether DRM is in use); the other reads a stream's playlist again from inside the frame that first loaded it, to learn its quality and length. Neither changes the page, neither leaves a script behind, and neither runs without the user's click.
```

**Host permission: <all_urls>**

```
Videos come from whatever servers a site uses (CDNs, player domains), so seeing which videos play with webRequest, and using scripting in the tab the user clicked on, needs access to all sites; any narrower pattern misses most of what users watch. The access is used only to recognise and list media. Cookies are read for youtube.com and tiktok.com, each only while the user has that site's session switch (YouTube session, TikTok session) on; another site's cookies are read only with "Other sites" on, only at the moment the user presses Get on that site's page, and only for that site's domain.
```

**Permission: cookies**

```
youtube.com and tiktok.com cookies, each only while the user has that site's switch in the popup ("YouTube session", "TikTok session") on; and with "Other sites" on, the cookies of the site whose page the user presses Get on, at that press only and for that site's domain only (never YouTube's or TikTok's). They are passed to Universal Downloader, which the user installed on the same computer, so its download engine can act as the signed-in user and download members-only YouTube videos the user pays for, age-restricted TikTok posts the user's own account can see, and on another site a post the user's own account there can see; the application keeps another site's cookies encrypted for at most an hour. Chrome 127 and later encrypt the cookie database against other local programs, so the application can no longer read it directly; chrome.cookies with native messaging is the supported path. No other cookies are read, and no other site's are ever read in the background.
```

**Permission: nativeMessaging**

```
The only channel this extension communicates over. It sends the addresses of the first few videos listed in the popup (so the application can say at what quality, in what format and at roughly what size each would download), the address of the video the user pressed Get on and — while their switches are on — the YouTube and TikTok sessions and the session of the site Get was pressed on to com.universaldownloader.bridge, a native messaging host installed by the Universal Downloader desktop application, using Chrome's stdio channel. Chrome starts that host itself and only for this extension's ID. Nothing listens on a network port; the extension has no server.
```

**Permission: storage**

```
Five small values in chrome.storage.local: a random identifier for this browser profile (so the desktop application can tell one Chrome profile from another), whether the user turned the YouTube session off, whether the user turned the TikTok session on, whether the user turned Other sites on, and what the application last said about this profile's connection (connected, turned off in the application, or not connected). In chrome.storage.session, in memory, each tab's list of the media it played, dropped when the tab or the browser closes. No cookie content is stored.
```

**Permission: alarms**

```
One alarm, once a day, which re-sends the current session of each site whose switch is on (YouTube session, TikTok session), so the copy held by the desktop application does not expire. With both switches off, the alarm sends no cookies, and it never sends another site's, whatever Other sites says.
```

**Remote code:** No, I am not using remote code.

```
All logic is contained in the uploaded package. The extension loads no scripts from anywhere, evaluates nothing at runtime, and is not minified or obfuscated — background.js and media.js can be read as shipped. Both functions run with scripting are in the package.
```

**Data usage**

Tick Authentication information, Web history, User activity and Website content.

```
The addresses of the media tabs play are kept in the browser's session storage, to be listed in the extension's own popup, and dropped when the tab closes. While the popup is open, the details below are passed for the first few listed items to the same application, so it can say how each would download; it downloads and keeps nothing for this. When the user presses Get, only that item's address and kind, the page's address and title, the request's referrer and origin, the browser's user-agent string and the preview image's address are passed, over Chrome's native messaging channel, to a desktop application the user installed on the same computer. youtube.com and tiktok.com authentication cookies are passed the same way to the same application, each only while the user has that site's session switch on; with Other sites on, the cookies of the site whose page the user presses Get on are passed the same way at that press only, and the application keeps them for at most an hour. None of it is collected by the developer, transmitted to any server or shared with anyone.
```
