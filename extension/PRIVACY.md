# Privacy policy — Universal Downloader Connector

_Last updated: 21 September 2026_

This extension does not have a server. Nothing it reads is sent to the
developer, to any website, or to any third party. There is no analytics, no
telemetry and no account.

## What it reads, and why

The extension reads the cookies of **youtube.com** in the Chrome profile it is
installed in. Those cookies are what tell YouTube who you are, and the app needs
them for one thing: to download videos that your own YouTube membership already
gives you access to. Without them the download engine is an anonymous visitor
and a members-only video is closed to it.

If you press **Include my google.com sign-in** in the extension's popup, it also
reads **google.com** cookies. This is optional, it is never asked for at install
time, and it exists because on some accounts the part that YouTube checks lives
there instead.

## Where they go

To one place: a program called `ud-bridge.exe`, part of the Universal Downloader
desktop application, running on the same computer as your browser. Chrome starts
that program itself, over its native messaging channel, and only for this
extension. The extension makes no network requests of any kind — you can confirm
this by reading `background.js`, which is not minified or obfuscated.

The desktop application stores the session on your own disk, encrypted for your
Windows account, and hands it to the download engine only for the run that needs
it. It deletes it after seven days without a refresh, and immediately if you sign
out of YouTube.

## What the extension stores by itself

In `chrome.storage.local`, on your own machine:

- a random identifier for this browser profile, so the desktop app can tell one
  Chrome profile from another;
- whether you have switched the connection off;
- the last connection status, so the popup can show it.

No cookies and no browsing history are stored by the extension.

## The optional account hint

If you press **Show which account**, the extension asks Chrome for the email
address of the signed-in profile and masks it (`m•••@gmail.com`) before handing
it to the desktop app, so the app can show you which account it is about to use.
The full address never leaves the browser. This permission is optional and is
never requested at install time.

## Your control

- **Turn off** in the extension's popup stops it sending anything and tells the
  app to delete what it holds. It takes effect even while the app is closed.
- **Disconnect** in the app's Settings does the same from the other side.
- Removing the extension stops it entirely.

## What is never done

The data this extension handles is not sold, not transferred to anyone, not
used for advertising or creditworthiness, and not used for any purpose other
than the one described above.

## Contact

<!-- Put the address you want published here before submitting. The Chrome Web
     Store shows this to users, so it should be one you are willing to make
     public. -->

---

# Gizlilik politikası — Universal Downloader Connector

_Son güncelleme: 21 Eylül 2026_

Bu eklentinin sunucusu yok. Okuduğu hiçbir şey geliştiriciye, herhangi bir
siteye veya üçüncü bir tarafa gönderilmiyor. Analitik yok, telemetri yok, hesap
yok.

## Neyi, neden okuyor

Eklenti, kurulu olduğu Chrome profilindeki **youtube.com** çerezlerini okuyor.
O çerezler YouTube'a kim olduğunuzu söyleyen şeydir ve uygulamanın onlara tek
bir sebeple ihtiyacı var: kendi YouTube üyeliğinizin zaten erişim verdiği
videoları indirebilmek. Onlar olmadan indirme motoru isimsiz bir ziyaretçidir ve
üyelere özel bir video ona kapalıdır.

Popup'taki **google.com oturumumu da dahil et** düğmesine basarsanız
**google.com** çerezlerini de okur. Bu isteğe bağlıdır, kurulum sırasında asla
istenmez, ve bazı hesaplarda YouTube'un baktığı kısım orada durduğu için vardır.

## Nereye gidiyor

Tek bir yere: bilgisayarınızda çalışan, Universal Downloader masaüstü
uygulamasının parçası olan `ud-bridge.exe` adlı programa. O programı Chrome'un
kendisi başlatır, kendi yerel mesajlaşma kanalı üzerinden, yalnızca bu eklenti
için. Eklenti hiçbir ağ isteği yapmaz — küçültülmemiş ve gizlenmemiş olan
`background.js` dosyasını okuyarak bunu doğrulayabilirsiniz.

Masaüstü uygulaması oturumu kendi diskinizde, Windows hesabınıza şifreleyerek
saklar ve indirme motoruna yalnızca ihtiyaç duyulan çalıştırma için verir.
Yenilenmeden yedi gün geçerse siler; YouTube'dan çıkış yaparsanız hemen siler.

## Eklentinin kendi sakladıkları

Kendi makinenizdeki `chrome.storage.local` içinde:

- bu tarayıcı profili için rastgele bir tanımlayıcı, masaüstü uygulaması bir
  Chrome profilini diğerinden ayırabilsin diye;
- bağlantıyı kapatıp kapatmadığınız;
- popup'ın gösterebilmesi için son bağlantı durumu.

Eklenti hiçbir çerezi ve hiçbir gezinme geçmişini saklamaz.

## İsteğe bağlı hesap ipucu

**Hangi hesap olduğunu göster** düğmesine basarsanız eklenti Chrome'dan oturum
açmış profilin e-posta adresini ister ve masaüstü uygulamasına vermeden önce
maskeler (`m•••@gmail.com`), böylece uygulama hangi hesabı kullanacağını size
gösterebilir. Adresin tamamı tarayıcıdan çıkmaz. Bu izin isteğe bağlıdır ve
kurulum sırasında asla istenmez.

## Kontrol sizde

- Popup'taki **Kapat**, göndermeyi durdurur ve uygulamaya elindekini silmesini
  söyler. Uygulama kapalıyken bile geçerlidir.
- Uygulamanın Ayarlar bölümündeki **Bağlantıyı kes** aynı şeyi diğer taraftan
  yapar.
- Eklentiyi kaldırmak her şeyi tamamen durdurur.

## Asla yapılmayanlar

Bu eklentinin işlediği veri satılmaz, kimseye aktarılmaz, reklam veya kredi
değerlendirmesi için kullanılmaz ve yukarıda anlatılanın dışında hiçbir amaçla
kullanılmaz.

## İletişim

<!-- Yayımlamak istediğiniz adresi göndermeden önce buraya yazın. Chrome Web
     Store bunu kullanıcılara gösterir, yani herkese açık olmasına razı
     olduğunuz bir adres olmalı. -->
