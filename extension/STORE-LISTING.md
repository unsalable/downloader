# Chrome Web Store submission

Everything the dashboard asks for. Paste each block into the field it names.

**Package:** `extension-upload.zip`, from `node scripts/pack-extension.mjs`. It strips the
development `key` from the manifest, which is what Google's guidance says to do, and leaves
out `key.pem`, the icon generator and the developer notes. Bump `version` in
`extension/manifest.json` before every upload; Chrome refuses a package whose version has not
increased.

**Images:** `store-assets/`, from `node scripts/build-store-assets.mjs` (rerun it whenever the
popup or the mark changes). They are photographs of the real popup, rendered with its own HTML, CSS and
JavaScript — not mock-ups.

**Language.** The dashboard localizes the **listing** — name, summary, description, images —
once you add a language under Store listing, so both are given below. The **privacy tab** is
one value per field rather than one per language; nothing obliges those to be English, so they
are given in Turkish, which is the language of this account's dashboard. The English wording is
kept in the appendix for a reviewer exchange that happens in English.

---

## 1. Store listing — English

**Item name**

```
Universal Downloader Connector
```

**Summary**

```
Lets the Universal Downloader app on your computer use your YouTube sign-in, so you can download what your membership already gives you.
```

**Description**

```
Universal Downloader Connector is the browser half of Universal Downloader, a free desktop application for downloading media.

It does one thing. When you download a YouTube video that is only available to members of a channel you pay for, the download engine needs to be signed in as you. This extension hands your YouTube session to the Universal Downloader application running on the same computer, so the engine can see what you can see.

It exists because Chrome encrypts its cookie storage against other programs on the same machine. The application cannot read your session directly any more, and this extension is the supported way to pass it across: Chrome's own native messaging channel, from this extension to that application, on your machine.

WHAT IT DOES NOT DO

- It makes no network requests. None. It has no server, no analytics and no account.
- Nothing it reads leaves your computer. The only recipient is the Universal Downloader application, which you installed.
- It does not read, change or watch any web page.

HOW IT WORKS

1. Install Universal Downloader on your computer.
2. Add this extension. A page opens explaining the rest.
3. Press Connect. The app's Settings then shows which browser and which account it is connected to.

The popup shows what the app currently holds, and a Turn off button that stops everything and tells the app to delete the session. That works even while the app is closed.

GOOD TO KNOW

If the membership is on a second Google account you have added to the same Chrome profile, the exported session authenticates as the first account and YouTube will say you are not a member. Use a separate Chrome profile for that account.

Universal Downloader is open source: https://github.com/unsalable/downloader
```

**Category:** Workflow & Planning · **Language:** English

**Images:** `store-assets/en/1-connected.png`, `2-connect.png`, `3-privacy.png` (1280×800),
small promo tile `store-assets/en/promo-tile-440x280.png` (440×280).

---

## 2. Store listing — Türkçe

Mağaza girişi bölümünden dil seçiciyle Türkçe'yi ekleyip bunları doldurun.

**Öğe adı**

```
Universal Downloader Connector
```

**Özet**

```
Bilgisayarınızdaki Universal Downloader uygulamasının YouTube oturumunuzu kullanmasını sağlar; böylece üyeliğinizin zaten verdiği içeriği indirebilirsiniz.
```

**Açıklama**

```
Universal Downloader Connector, ücretsiz bir masaüstü indirme uygulaması olan Universal Downloader'ın tarayıcı tarafıdır.

Tek bir iş yapar. Para verip üye olduğunuz bir kanalın yalnızca üyelere açık videosunu indirmek istediğinizde, indirme motorunun sizin olarak oturum açmış olması gerekir. Bu eklenti, YouTube oturumunuzu aynı bilgisayarda çalışan Universal Downloader uygulamasına verir; böylece motor da sizin gördüğünüzü görür.

Var olma sebebi şu: Chrome, çerez deposunu aynı makinedeki diğer programlara karşı şifreliyor. Uygulama oturumunuzu artık doğrudan okuyamıyor ve bu eklenti, onu karşıya geçirmenin desteklenen yolu: Chrome'un kendi yerel mesajlaşma kanalı, bu eklentiden o uygulamaya, sizin makinenizde.

YAPMADIKLARI

- Hiçbir ağ isteği yapmaz. Hiç. Sunucusu, analitiği ve hesabı yoktur.
- Okuduğu hiçbir şey bilgisayarınızdan çıkmaz. Tek alıcı, sizin kurduğunuz Universal Downloader uygulamasıdır.
- Hiçbir web sayfasını okumaz, değiştirmez veya izlemez.

NASIL ÇALIŞIR

1. Universal Downloader'ı bilgisayarınıza kurun.
2. Bu eklentiyi ekleyin. Gerisini anlatan bir sayfa kendiliğinden açılır.
3. Bağlan'a basın. Uygulamanın Ayarlar bölümü hangi tarayıcıya ve hangi hesaba bağlı olduğunu gösterir.

Popup, uygulamanın elinde şu an ne olduğunu gösterir ve bir Kapat düğmesi sunar: her şeyi durdurur ve uygulamaya oturumu silmesini söyler. Bu, uygulama kapalıyken de çalışır.

BİLMEKTE FAYDA VAR

Üyelik, aynı Chrome profiline eklediğiniz ikinci bir Google hesabındaysa, dışa aktarılan oturum ilk hesap olarak kimlik doğrular ve YouTube üye olmadığınızı söyler. O hesap için ayrı bir Chrome profili kullanın.

Universal Downloader açık kaynaktır: https://github.com/unsalable/downloader
```

**Kategori:** Workflow & Planning · **Dil:** Türkçe

**Görseller:** `store-assets/tr/1-connected.png`, `2-connect.png`, `3-privacy.png` (1280×800),
küçük tanıtım karosu `store-assets/tr/promo-tile-440x280.png` (440×280).

---

## 3. Gizlilik uygulamaları — alan başına tek değer

Bunlar dile göre çoğalmaz: her alan tek bir değer alır ve onu inceleyen bir kişi okur.

**Tek amaç**

```
Kullanıcının YouTube oturumunu, aynı bilgisayarda çalışan ve yine kullanıcının kendisine ait olan Universal Downloader masaüstü uygulamasına aktarmak; böylece uygulama, kullanıcının üyeliğinin zaten erişim verdiği videoları indirebilir.
```

**İzin: cookies**

```
Eklentinin var oluş sebebi budur. Eklenti, kullanıcının youtube.com çerezlerini okur ve bunları aynı bilgisayara kullanıcının kendisinin kurduğu masaüstü uygulaması olan Universal Downloader'a aktarır; böylece uygulamanın indirme motoru oturum açmış kullanıcı olarak davranabilir ve kullanıcının parasını ödediği, yalnızca üyelere açık videoları indirebilir.

Çerezler başka hiçbir yere gönderilmez. Bu eklenti hiçbir ağ isteği yapmaz; tek alıcı, aynı makinede çalışan yerel mesajlaşma sunucusudur. Bunu yapabilecek başka bir API yoktur: Chrome 127 ve sonrası çerez veritabanını aynı makinedeki diğer programlara karşı şifrelediği için uygulama onu artık doğrudan okuyamamakta, yerel mesajlaşma ile birlikte kullanılan chrome.cookies ise bunun desteklenen yoludur.
```

**İzin: nativeMessaging**

```
Bu eklentinin iletişim kurduğu tek kanaldır. Oturumu, Universal Downloader masaüstü uygulamasının kurduğu com.universaldownloader.bridge adlı yerel mesajlaşma sunucusuna, Chrome'un stdio kanalı üzerinden gönderir. O sunucuyu Chrome'un kendisi başlatır ve yalnızca bu eklentinin kimliği için başlatır. Hiçbir ağ portu dinlenmez; eklenti kendi başına hiçbir bağlantı açmaz.
```

**İzin: storage**

```
chrome.storage.local içinde üç küçük değer saklar: bu tarayıcı profili için rastgele bir tanımlayıcı — masaüstü uygulaması bir Chrome profilini diğerinden ayırabilsin ve kullanıcının bağlamadığı bir profilden gelen oturumu reddedebilsin diye; kullanıcının bağlantıyı kapatıp kapatmadığı bilgisi; ve popup'ın gösterebilmesi için son bağlantı durumu. Hiçbir gezinme verisi ve hiçbir çerez içeriği saklanmaz.
```

**İzin: alarms**

```
Günde bir kez çalışan tek bir alarm, mevcut oturumu yeniden göndererek masaüstü uygulamasının elindeki kopyanın eskimesini önler. Bu olmasa, kullanıcının haftalar önce bağladığı bir oturum sessizce geçerliliğini yitirir ve yalnızca üyelere açık indirmeler, sebebini açıklayan hiçbir şey olmadan başarısız olmaya başlardı.
```

**Ana makine izni: https://*.youtube.com/***

```
chrome.cookies, çerezleri okunan alan adı için ana makine izni gerektirir. Bu özelliğin ihtiyaç duyduğu tek alan adı youtube.com olduğu için kurulum sırasında istenen tek izin odur. Eklenti hiçbir sayfaya betik eklemez ve hiçbir sayfa içeriğini okumaz.
```

**İsteğe bağlı izin: identity, identity.email**

```
Yalnızca kullanıcı popup'taki "Hangi hesap olduğunu göster" düğmesine basarsa istenir; kurulum sırasında asla istenmez. Tek bir amaçla kullanılır: oturum açmış profilin e-posta adresini okumak. Adres, masaüstü uygulamasında gösterilmeden önce eklenti içinde maskelenir (ilk karakter, ardından noktalar, ardından alan adı); böylece iki profili olan bir kullanıcı hangi hesabın kullanılmak üzere olduğunu doğrulayabilir. Adresin tamamı tarayıcıdan çıkmaz.

identity izni identity.email ile birlikte listelenmiştir; çünkü chrome.identity.getProfileUserInfo temel iznin var olmasını gerektirir.
```

**İsteğe bağlı ana makine izni: https://*.google.com/***

```
Yalnızca popup üzerinden ve yalnızca tek başına youtube.com çerezleriyle oturum açma denemesi başarısız olduktan sonra istenir. Bazı hesaplarda YouTube'un kontrol ettiği oturum çerezi google.com üzerinde tutulur. Kurulum sırasında asla istenmez ve kullanıcıların çoğu için özellik bu izin olmadan da çalışır.
```

**Uzaktan kod:** Hayır, uzaktan kod kullanmıyorum.

```
Tüm mantık yüklenen paketin içindedir. Eklenti hiçbir yerden betik yüklemez, çalışma zamanında hiçbir kod değerlendirmez ve küçültülmemiş ya da gizlenmemiştir; background.js yayınlandığı haliyle okunabilir.
```

**Veri kullanımı**

**Kimlik doğrulama bilgileri** kutusunu işaretleyin. Diğer kategorilerin hiçbirini
işaretlemeyin: eklenti kişisel iletişim, konum, web geçmişi, gezinme etkinliği veya kullanıcı
içeriğine dokunmaz.

Verinin ne için kullanıldığını soran kutuya:

```
Kimlik doğrulama çerezleri, Chrome'un yerel mesajlaşma kanalı üzerinden, kullanıcının aynı bilgisayara kendisinin kurduğu bir masaüstü uygulamasına yerel olarak aktarılır. Geliştirici tarafından toplanmaz, hiçbir sunucuya iletilmez ve kimseyle paylaşılmaz. Bu eklenti hiçbir ağ isteği yapmaz.
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

## 4. Onaylandıktan sonra

Mağaza kendi eklenti kimliğini üretir ve geliştirme `key` alanını yok sayar; dolayısıyla
yayımlanan eklentinin kimliği, paketsiz kurulanınkinden **farklıdır**. Masaüstü uygulaması
tanımadığı hiçbir kimliğe cevap vermediği için bu adım isteğe bağlı değildir:

1. Kimliği panodan (veya listenin adresinden) kopyalayın.
2. `src-tauri/src/bridge/protocol.rs` içindeki `EXTENSION_ID_STORE` değerini ona eşitleyin.
3. Yeniden derleyip sürüm çıkarın.

`EXTENSION_ID_DEV` olduğu yerde kalır, böylece geliştirme kurulumu yayımlanan sürümle yan yana
çalışmaya devam eder. `EXTENSION_ID_STORE` ayarlandığında `store_listed()` de doğru olur ve
uygulamanın Bağlantı ayarlarındaki "henüz yayımlanmadı" notunun yerini listeyi açan düğme alır.

`extension/key.pem` dosyasını yeniden üretmeyin. Geliştirme kimliğini o sabitler; yenisi,
`EXTENSION_ID_DEV` güncellenene kadar host'un geliştirme derlemelerine cevap vermesini sessizce
durdurur.

---

## Ek — the same privacy answers in English

Kept for a review exchange that happens in English, or if the dashboard language changes.
These are the same statements as section 3, not different ones.

**Single purpose**

```
Passing the user's YouTube sign-in to the user's own copy of the Universal Downloader desktop application, running on the same computer, so it can download videos the user's membership already grants access to.
```

**Permission: cookies**

```
This is the feature. The extension reads the user's youtube.com cookies and passes them to Universal Downloader, a desktop application the user installed on the same computer, so its download engine can act as the signed-in user and download members-only videos the user pays for.

The cookies are not sent anywhere else. This extension makes no network requests at all; the only recipient is a native messaging host on the same machine. No other API can do this: Chrome 127 and later encrypt the cookie database against other local programs, so the application can no longer read it directly, and chrome.cookies with native messaging is the supported path.
```

**Permission: nativeMessaging**

```
The only channel this extension communicates over. It sends the session to com.universaldownloader.bridge, a native messaging host installed by the Universal Downloader desktop application, using Chrome's stdio channel. Chrome starts that host itself and only for this extension's ID. Nothing listens on a network port and the extension opens no connections of its own.
```

**Permission: storage**

```
Stores three small values in chrome.storage.local: a random identifier for this browser profile, so the desktop application can tell one Chrome profile from another and refuse a session from a profile the user did not connect; whether the user has switched the connection off; and the last connection status, so the popup can display it. No browsing data and no cookie content is stored.
```

**Permission: alarms**

```
One alarm, once a day, which re-sends the current session so the copy held by the desktop application does not expire. Without it a session the user connected weeks ago would silently go stale and members-only downloads would start failing with nothing to explain why.
```

**Host permission: https://*.youtube.com/***

```
chrome.cookies requires host permission for the domain whose cookies are read. youtube.com is the only domain the feature needs, so it is the only one requested at install time. The extension does not inject scripts into any page or read page content.
```

**Optional permission: identity, identity.email**

```
Requested only if the user presses "Show which account" in the popup, never at install time. It is used once, to read the signed-in profile's email address, which is masked in the extension (first character, then dots, then the domain) before being shown in the desktop application, so a user with two profiles can confirm which account is about to be used. The full address never leaves the browser.

identity is listed alongside identity.email because chrome.identity.getProfileUserInfo needs the base permission to exist at all.
```

**Optional host permission: https://*.google.com/***

```
Requested only from the popup, and only after a sign-in has already failed with youtube.com cookies alone. On some accounts the session cookie YouTube checks is held on google.com instead. It is never requested at install time and the feature works without it for most users.
```

**Remote code:** No, I am not using remote code.

```
All logic is contained in the uploaded package. The extension loads no scripts from anywhere, evaluates nothing at runtime, and is not minified or obfuscated — background.js can be read as shipped.
```

**Data usage**

```
The authentication cookies are transferred locally, over Chrome's native messaging channel, to a desktop application the user installed on the same computer. They are not collected by the developer, not transmitted to any server, and not shared with anyone. This extension makes no network requests.
```
