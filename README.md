# csengo

az iskolai stúdió munkáját segítő program, ami a csengetések, és rádiós adások
automatizálására szolgál. az előző megoldással ellentétben, ez a projekt
cross-platform, és a lehető legkevesebb rendszerkövetelmény egy fontos szempont
volt, annak érdekében, hogy minél több környezetben használható legyen.

## működés

###### _ezt főleg azért írom le, hogy hátha évekkel az elballagásom után is ez a rendszer maradna életben, könnyebb dolga legyen annak a szerencsétlennek, akinek előbb vagy utóbb foglalkoznia kell ezzel._

maga a fő program, [rust](https://rust-lang.org)-ban készült, ez felel a
lejátszásért, és egy web szervert is futtat, amivel elérhető lesz a webes admin
felület, illetve az API, amivel irányítani lehet.

a webes felületet a szerver generálja a [templates](templates) mappában lévő minták alapján.
a weboldal [htmx](https://htmx.org) segítségével kommunikál a szerverrel.
a [frontend](frontend) mappában lévő kis kódrészek alapján egy js és egy css
fálj készül minden kóddal, ami a frontend-re kell,
amik statikusan a csengőprogram végső executable fileba lesznek bemásolva.

avagy maga a program egyetlen egy file, kizárólag ezt kell átmásolni a
számítógépre, amin futni fog. ez lehet egy teljes windows 10 gép, vagy akár egy
raspberry pi.

a csengetéseket egy sqlite adatbázisban tárolja, hogy az egyszeri és ismétlődő
időzített csengetések újraindításkor ne vesszenek el. a feltöltött audio fájlok
is itt vannak tárolva.

a gördülékeny fejlesztés érdekében Github Actions scripteket is írtam, amik
x86_64 windows és linux, valamint armv7 és arm64 linux platformokra
automatikusan buildelnek.

Ha manuálisan buildelnéd, nézd meg a scripteket, de nagyjából ezek fognak
kelleni:

-   rust, cargo
-   nodejs, pnpm
-   linuxon alsa header fileok (`libasound2-dev`)

## setup

Telepítési instrukciók Debian Linux alapú rendszerekre user service-ként

*(elméletileg bármilyen distro-n működik amíg systemd-t használ)*

```sh
# a source kód letöltése, a programot nem most build-eljük,
# az már megvan (elvileg), de a config fájlok és scriptek kellenek
cd ~
git clone https://github.com/beni69/csengo

cd csengo
chmod +x misc/update.sh
./misc/update.sh # letölti a legújabb verziót

# systemd service beállítása és elindítása
cd misc
mkdir -vp ~/.config/systemd/user/
ln -sv $PWD/csengo.service ~/.config/systemd/user/
systemctl daemon-reload
systemctl --user enable --now csengo.service
systemctl --user status csengo.service
```

## todo

- [ ] task import
- [ ] use [systemd credentials](https://www.freedesktop.org/software/systemd/man/systemd.exec.html#Credentials) instead of an env file
