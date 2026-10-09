# Privacy policy — Universal Downloader Connector

_Last updated: 8 October 2026_

This extension does not have a server. Nothing it sees is sent to the
developer, to any website, or to any third party. There is no analytics, no
telemetry and no account. The only thing it ever sends anything to is the
Universal Downloader application on your own computer.

## What it looks at, and why

**The video and audio your tabs play.** To list them, the extension watches
the network requests your tabs make and looks at the ones that carry video or
audio: their address, the type and size the server reports, and which page and
frame asked for them. Other requests are looked at only long enough to see
that they are not media. What it keeps is held in the browser's session
storage, in memory: the list for a tab is dropped when the tab moves to another
page or closes, and all of it is gone when the browser closes.

**The page you open the popup on.** When you click the toolbar icon, the
extension reads the current tab's title, its preview image address, and its
`<video>` and `<audio>` elements (their address, poster, length and size, and
whether the site protects them with DRM). To learn a stream's quality and
length it may read the stream's playlist again, from inside the page that
loaded it -- the same request the page made, often answered from the browser's
cache. The popup shows the page's preview images, loaded from the site that
serves them.

**Your sign-ins, each only if you turn it on.** With the **YouTube session**
switch on, the extension reads the cookies of **youtube.com**, so the app can
download members-only videos your own membership gives you. With the **TikTok
session** switch on, it reads the cookies of **tiktok.com**, so the app can
download age-restricted posts your own TikTok account can see. With the
**Other sites** switch on, and only at the moment you press **Get** on a page
of some other site -- Instagram, X, Facebook, Vimeo, Reddit and so on -- it
reads that one site's cookies (the page's own domain, such as instagram.com,
and its subdomains), so the app can download a post your own account on that
site can see. Nothing else is read: no cookies of a site you did not press
Get on, and none in the background. Each switch is off until you turn it on,
and turning one on does not turn on another.

## What it sends, and where

To one place: a program called `ud-bridge.exe`, part of the Universal Downloader
desktop application, running on the same computer as your browser. Chrome starts
that program itself, over its native messaging channel, and only for this
extension.

- **When the popup lists a video:** the same details as below, for each of the
  first few items it lists, so the app can say what it would download -- the
  title, the picture, the quality, the format and roughly the size. The app
  reads the video's page or stream from the site to answer, as it would before
  downloading; it downloads nothing and keeps nothing for this.
- **When you press Get (İndir in Turkish):** that one item's address and kind, the
  page's title and address, the referring address and origin the request was
  made with, your browser's user-agent string and the preview image address.
  The app opens the video, and when you have chosen the quality and the
  watermark and pressed download there, downloads it from the site, as your
  browser would.
  Nothing is downloaded for items you do not press.
- **When you open the popup:** a question -- is the app installed, and is this
  profile the connected one? It carries the profile identifier below, the
  browser's name and the extension's version, and nothing about the page. The
  same question goes before the extension sends your cookies on its own
  whenever it is not sure the session is still switched on in the app.
- **While the YouTube session is on:** your youtube.com cookies, again when
  they change, when you open the popup and once a day. The app stores them on
  your disk encrypted for your Windows account, hands them to the download
  engine only for the download that needs them, and deletes them when you turn
  the switch off, after seven days without a refresh, or when you sign out of
  YouTube.
- **While the TikTok session is on:** your tiktok.com cookies, again when your
  TikTok sign-in changes, when you open the popup, when you press Get on a
  TikTok address and once a day. The app keeps only tiktok.com's, apart from
  YouTube's and encrypted the same way, hands them to the download engine only
  for a TikTok download that needs them, and deletes them when you turn the
  switch off, after seven days without a refresh, or when you sign out of
  TikTok.
- **When you press Get with Other sites on:** the cookies of that page's site,
  just before the item itself, and at no other time -- not when they change,
  not when you open the popup, not once a day. The app keeps only that
  site's, encrypted the same way, hands them to the download engine only for
  a download from that site, and deletes them an hour after they were sent,
  when you press Get on another site (which replaces them), or when you turn
  the switch off. If the browser has no cookies for that site, nothing is
  kept, and what was held for the site before goes.

You can confirm all of this by reading `background.js`, which is not minified
or obfuscated.

## What the extension stores by itself

In `chrome.storage.local`, on your own machine: a random identifier for this
browser profile (so the app can tell one Chrome profile from another), whether
you turned the YouTube session off, whether you turned the TikTok session and
Other sites on, and what the app last said about this profile's connection
(connected, turned off in the app, or not connected).

In `chrome.storage.session`, in memory: the per-tab lists described above.

No cookies are stored by the extension, and no browsing history is kept beyond
the open tabs.

## Your control

- Nothing about the pages you visit leaves the browser until you press
  **Get**, and your cookies leave it only while the **YouTube session**,
  **TikTok session** or **Other sites** switch is on -- and only that site's.
  An other site's leave it only when you press Get on its page.
- Turning a switch off stops that site's cookies being sent and tells the app
  to delete what it holds of them, even if the app is closed at the time.
- Removing the extension stops it entirely.

## What is never done

The data this extension handles is not sold, not transferred to anyone, not
used for advertising or creditworthiness, and not used for any purpose other
than the one described above. DRM-protected media is never circumvented: the
extension marks it as protected and offers no way to download it.

## Contact

<!-- Put the address you want published here before submitting. The Chrome Web
     Store shows this to users, so it should be one you are willing to make
     public. -->

---

# Gizlilik politikası — Universal Downloader Connector

_Son güncelleme: 8 Ekim 2026_

Bu eklentinin sunucusu yok. Gördüğü hiçbir şey geliştiriciye, herhangi bir
siteye veya üçüncü bir tarafa gönderilmiyor. Analitik yok, telemetri yok, hesap
yok. Bir şey gönderdiği tek yer, kendi bilgisayarınızdaki Universal Downloader
uygulamasıdır.

## Neye bakıyor, neden

**Sekmelerinizin oynattığı video ve sesler.** Bunları listelemek için eklenti,
sekmelerinizin yaptığı ağ isteklerini izler ve video ya da ses taşıyanlara
bakar: adreslerine, sunucunun bildirdiği türe ve boyuta, onları hangi sayfanın
ve çerçevenin istediğine. Diğer isteklere yalnızca medya olmadıklarını görecek
kadar bakılır. Tuttukları tarayıcının oturum deposunda, bellekte durur: bir
sekmenin listesi sekme başka bir sayfaya geçince ya da kapanınca silinir,
tarayıcı kapanınca hepsi gider.

**Açılır pencereyi açtığınız sayfa.** Araç çubuğundaki simgeye tıkladığınızda
eklenti, geçerli sekmenin başlığını, önizleme görselinin adresini ve
`<video>`/`<audio>` öğelerini (adreslerini, kapak görsellerini, sürelerini ve
boyutlarını, sitenin onları DRM ile koruyup korumadığını) okur. Bir akışın
kalitesini ve süresini öğrenmek için akışın oynatma listesini, onu yükleyen
sayfanın içinden yeniden okuyabilir -- sayfanın yaptığı isteğin aynısı; çoğu
zaman tarayıcının önbelleğinden yanıtlanır. Açılır pencere, sayfanın önizleme
görsellerini onları sunan siteden yükleyerek gösterir.

**Oturumlarınız, her biri yalnızca siz açarsanız.** **YouTube oturumu**
anahtarı açıkken eklenti **youtube.com** çerezlerini okur; böylece uygulama
kendi üyeliğinizin erişim verdiği üyelere özel videoları indirebilir.
**TikTok oturumu** anahtarı açıkken **tiktok.com** çerezlerini okur; böylece
uygulama kendi TikTok hesabınızın görebildiği yaş sınırlı gönderileri
indirebilir. **Diğer siteler** anahtarı açıkken, yalnızca başka bir sitenin
-- Instagram, X, Facebook, Vimeo, Reddit gibi -- sayfasında **İndir**'e
bastığınız anda, o tek sitenin çerezlerini (sayfanın kendi alan adının,
örneğin instagram.com'un, ve alt alan adlarının çerezlerini) okur; böylece
uygulama o sitedeki kendi hesabınızın görebildiği bir gönderiyi indirebilir.
Başka hiçbir şey okunmaz: İndir'e basmadığınız bir sitenin çerezleri de,
arka planda hiçbir çerez de. Her anahtar siz açana kadar kapalıdır ve birini
açmak diğerlerini açmaz.

## Neyi, nereye gönderiyor

Tek bir yere: bilgisayarınızda çalışan, Universal Downloader masaüstü
uygulamasının parçası olan `ud-bridge.exe` adlı programa. O programı Chrome'un
kendisi başlatır, kendi yerel mesajlaşma kanalı üzerinden, yalnızca bu eklenti
için.

- **Açılır pencere bir video listelediğinde:** listelediği ilk birkaç öğenin
  her biri için aşağıdakiyle aynı bilgiler; böylece uygulama ne indireceğini
  söyleyebilir -- başlığı, görseli, kaliteyi, biçimi ve yaklaşık boyutu.
  Uygulama bunu yanıtlamak için videonun sayfasını ya da akışını, indirmeden
  önce yapacağı gibi siteden okur; bunun için hiçbir şey indirmez, hiçbir şey
  saklamaz.
- **İndir'e bastığınızda:** yalnızca o öğenin adresi ve türü, sayfanın başlığı
  ve adresi, isteğin yapıldığı yönlendiren adres ve köken, tarayıcınızın
  kullanıcı aracısı dizesi ve önizleme görselinin adresi. Uygulama videoyu
  açar; kaliteyi ve filigranı seçip indirmeyi seçtiğinizde, tarayıcınızın
  yapacağı gibi siteden indirir. Basmadığınız öğeler indirilmez.
- **Açılır pencereyi açtığınızda:** bir soru -- uygulama kurulu mu, bağlı
  profil bu mu? Aşağıdaki profil tanımlayıcısını, tarayıcının adını ve
  eklentinin sürümünü taşır; sayfayla ilgili hiçbir şey taşımaz. Eklenti
  çerezlerinizi kendiliğinden göndermeden önce, oturumun uygulamada hâlâ açık
  olduğundan emin değilse aynı soruyu sorar.
- **YouTube oturumu açıkken:** youtube.com çerezleriniz; değiştiklerinde,
  açılır pencereyi açtığınızda ve günde bir kez yeniden. Uygulama onları
  diskinizde Windows hesabınıza özel şifreleyerek saklar, indirme motoruna
  yalnızca onlara ihtiyaç duyan indirme için verir; anahtarı kapattığınızda,
  yedi gün yenilenmezse ya da YouTube'dan çıkış yaparsanız siler.
- **TikTok oturumu açıkken:** tiktok.com çerezleriniz; TikTok oturumunuz
  değiştiğinde, açılır pencereyi açtığınızda, bir TikTok adresinde İndir'e
  bastığınızda ve günde bir kez yeniden. Uygulama yalnızca tiktok.com'a ait
  olanları tutar, onları YouTube'unkilerden ayrı ve aynı şekilde şifreli
  saklar, indirme motoruna yalnızca onlara ihtiyaç duyan bir TikTok indirmesi
  için verir; anahtarı kapattığınızda, yedi gün yenilenmezse ya da TikTok'tan
  çıkış yaparsanız siler.
- **Diğer siteler açıkken İndir'e bastığınızda:** o sayfanın sitesinin
  çerezleri, öğenin kendisinden hemen önce ve başka hiçbir zaman --
  değiştiklerinde de, açılır pencereyi açtığınızda da, günde bir kez de
  gönderilmez. Uygulama yalnızca o siteye ait olanları tutar, onları aynı
  şekilde şifreli saklar, indirme motoruna yalnızca o siteden bir indirme için
  verir; gönderildikten bir saat sonra, başka bir sitede İndir'e bastığınızda
  (yenileri eskilerin yerini alır) ya da anahtarı kapattığınızda siler. O
  site için tarayıcıda hiç çerez yoksa hiçbir şey tutulmaz, önceden tutulan
  da silinir.

Bunların hepsini, küçültülmemiş ve gizlenmemiş olan `background.js` dosyasını
okuyarak doğrulayabilirsiniz.

## Eklentinin kendi sakladıkları

Kendi makinenizdeki `chrome.storage.local` içinde: bu tarayıcı profili için
rastgele bir tanımlayıcı (uygulama bir Chrome profilini diğerinden ayırabilsin
diye), YouTube oturumunu kapatıp kapatmadığınız, TikTok oturumunu ve Diğer
siteler anahtarını açıp açmadığınız ve uygulamanın bu profilin bağlantısı
hakkında en son ne dediği (bağlı, uygulamada kapalı ya da bağlı değil).

`chrome.storage.session` içinde, bellekte: yukarıda anlatılan sekme listeleri.

Eklenti hiçbir çerezi saklamaz ve açık sekmelerin ötesinde bir gezinme geçmişi
tutmaz.

## Kontrol sizde

- Ziyaret ettiğiniz sayfalarla ilgili hiçbir şey, İndir'e basana kadar
  tarayıcıdan çıkmaz; çerezleriniz yalnızca **YouTube oturumu**, **TikTok
  oturumu** ya da **Diğer siteler** anahtarı açıkken ve yalnızca o sitenin
  olanları çıkar. Diğer bir sitenin çerezleri yalnızca onun sayfasında
  İndir'e bastığınızda çıkar.
- Bir anahtarı kapatmak o sitenin çerezlerinin gönderilmesini durdurur ve
  uygulamaya o siteden elindekini silmesini söyler; uygulama o sırada kapalı
  olsa bile.
- Eklentiyi kaldırmak her şeyi tamamen durdurur.

## Asla yapılmayanlar

Bu eklentinin işlediği veri satılmaz, kimseye aktarılmaz, reklam veya kredi
değerlendirmesi için kullanılmaz ve yukarıda anlatılanın dışında hiçbir amaçla
kullanılmaz. DRM ile korunan medya asla aşılmaz: eklenti onu korumalı olarak
işaretler ve indirmek için hiçbir yol sunmaz.

## İletişim

<!-- Yayımlamak istediğiniz adresi göndermeden önce buraya yazın. Chrome Web
     Store bunu kullanıcılara gösterir, yani herkese açık olmasına razı
     olduğunuz bir adres olmalı. -->
