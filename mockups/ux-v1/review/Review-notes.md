Hier sind Dinge die mir aufgefallen sind beim austesten des Mockups:

Ich habe versucht das Mockup so gut es geht zu benutzen und zu beschreiben, es ist. Viel text geworden und ich bin mir auch bei einigen dingen nicht ganz sicher aber die Richtung gefällt mir insgesamt gut.
Manche meiner punkte sind auch eher offene punkte oder punkte die zu prüfen sind / zu Challangen sind.

# Filmstreifen

￼![alt text](filmstrip.png)

- Ich glaube der Dateiname reicht als zusätzliche info zum Bild also die zweite Zeile raus, damit sollte es aufgeräumter wirken, zusätzlich sollte der Dateiname so wie jetzt auch schon nur eine Zeile in Anspruch nehmen damit sichergestellt ist das ein Eintrag in dem Filmstreifen immer exakt gleich groß ist so entsteht der Effekt wie man ihn im bild sieht nämlich nicht.
- Die Markierung die anzeigt ob das bild bearbeitet wurde oder nicht sollte nur sichtbar sein wenn es bearbeitet wurde, ansonsten braucht es nicht sichtbar zu sein.
- Die extra infos die in der 2. Zeile sind aktuell könnte man + eventuell weitere wichtige metadaten als tooltip beim hover stilvoll anzeigen.

# Header

￼![alt text](header.png)

- Ich bin mir nicht sicher ob das mit dem save nach jeder Änderung klappen wird bei so großen Bildern technisch gesehen das müsstest du evaluieren da die Veränderung berechnen vermutlich etwas zeitaufwändiger werden wird.
- Die meta informationen finde ich hilfreich aber etwas lang und schwer scannbar visuell als user
- Wo finden sich eigentlich informationen wie Import, export, undo/redo,

# Toolsettings

￼![alt text](tool-settings.png)

- Toolsettings sind scheinbar immer oben in der Mitte wenn ich es richtig verstehe, hier müsste man vermutlich sicherstellen das die leiste immer gleich hoch ist
- Wenn Keyboard shortcuts gezeigt werden ist es hilfreich sie wie tasten zu stylen wie man das aus anderen Anwendungen kennt
- Vorne steht der toolname vielleicht würde da das icon zusätzlich noch helfen um den Bezug besser herzustellen, generell finde ich das der Bezug etwas besser rüberkommen müsste das dass jetzt die Settings sind von dem tool das ich grade ausgewählt habe also das meine Aktion das verursacht hat und dort kann ich jetzt dinge machen bzw muss ich dinge machen bei manchen tools wie fill bestätigen.
- Ux tipps bei inputs, Doppelklick auf den Input setzt ihn zurück auf default, increments in sinnvollen Steps, alternativ als extra shift für größere stepsize
- Was passiert wenn ein tool zu viele Controls hat so das der platz nicht mehr reicht? Oder sollte man schauen das sowas einfach nicht vorkommt? Wie geht man damit um?

# Navigator

￼![alt text](navigator.png)

- Ich finde gut das es oben rechts die shortcuts gibt um die sichten anzufahren
- Es ist auch sehr gut das man den aktuellen Bereich dort visualisiert sieht den man grade betrachtet
- Ebenso finde ich es sehr gut das man dort auch den Bereich anklicken kann im bild um die Sicht zu bewegen
- Eben so sind die beiden anderen Projektionen finde ich nett, genau so wie die live Koordinaten die sind auch gut
- Bei den live koordinaten wäre es aber gut wenn man die sogar eingeben könnte um immer eine exakte Sicht gezielt und präzise anzufahren
- Ich finde jedoch auch das der Navigator sehr viel in sehr wenig fläche packt und dadurch etwas überladen wirkt jetzt schon
- Wenn man ebenen hovert werden diese dort auch gehighlighted angezeigt das finde ich super gut, genau so wie die bearbeiteten Bereiche da weiß man direkt wo man was gemacht hat und sogar welche ebene wo was gemacht hat, super!

# Ebenen

￼![alt text](ebenen.png)

- Oben rechts das non-desctructive kann denke ich weg
- Das ebenen hovern in der navgator Ansicht und im bild highlighted wo was gemacht wurde ist super
- Es fehlt die Option eine ebene zu löschen!
- Ordner die benennbar sind wären gut
- Generell finde ich müsste das mehr in die Richtung gehen von den Funktionen her wie Photoshop ebenen also mit umsortieren und Masken und adjustment layern, hier müsste man ausarbeiten was man alles wann braucht.
- Ich habe hier generell etwas Probleme zu beschreiben wo wir am besten hin wollen in Sachen Feature fülle.

# Tools

￼![alt text](tools.png)

- Die masked brush ist an sich gut, ein nettes extra wäre wenn man einen Punkt mit einem klick irgendwo im bild macht und dann hält man shift gedrückt und macht einen zweiten klick wo anders das die punkte dann mit einer graden Linie verbunden werden, ich glaube Photoshop macht das auch so.
- Ein Objekt auswählen tool wäre super jedoch weiß ich hier technisch nicht wie man sowas in gut umsetzt, vermutlich auch mit einem Model?
- Falls man mit „wenig“ edit Funktionen auskommen will müssten die schlauen Wege sehr gut zuverlässig funktionieren
- Generell sollten wir überlegen in wie weit die editier Funktionen ausgebaut werden denn z.b ist ein häufiger usecase in Photoshop auch das man seine auswahlen refined bzw verbessert und nicht einfach nur grob pinselt.
- Das aufhellen Werkzeug funktioniert zumindest in der Demo hier nur semi gut, müsste man schauen wie gut das später funktioniert (es hellt nur auf wodurch der schatten grau wird)
- Im Pinselmodus könnte man auch immer per hotkey subtrahieren
- Bei dem clonestamp tool finde ich muss man das auswählen der source etwas besser an den user bringen das finde ich auch in Photoshop sub-optimal.
- Bei dem blur wären alternative noch ein Gauss blur gut zum verfeinern von dingen
- Das nadirpatch tool finde ich super da bräuchte man natürlich noch die Möglichkeit sein eigenes logo rein zu laden oder die farbe/muster zu wählen wenn man es einfach gefüllt haben will, diese blurred floor Option ist auch gut die sieht man oft in streetview touren.rotation und große passen auch und das snapping mit shift ist super.
- Das „Default Tool“ sollte immer das navigate/move tool sein, da es ein 360° bild ist ist meine Annahme das der user sich direkt umschauen möchte.

# Marking menu

￼![alt text](marking-menu.png)

- Ich finde das geht schon stark in die richtige Richtung mit dem markig Menu. Die abstände sind nicht ganz optimal also es gibt hier und da etwas overlap
- Man könnte Aktionen besser unterscheiden, Maya macht das z.b indem sie ihren icons färben geben je nach Domäne, also modelling ist z.b immer tool icons die orange mit drinnen haben, UV mapping hat immer grün in seinen icons, motion graphics MASH immer einen rot ton usw, das Konzept lässt sich vermutlich hier auch gut anwenden mit z.b Navigation, edit tools usw.
- Maya löst auch das Problem für komplexere tools wo Konfiguration oft einher geht z.b beim Nadir patch wo man vermutlich das logo hinterlegen will wo sich dann ein Settings Dialog oder sowas öffnet. In Maya sind das kleine boxen auf der tool Kachel und wenn man die box anfährt öffnet man die Settings von dem tool und wenn man das tool anfährt ohne die box zu treffen geht man ins tool. Ich denke generell kann man sich viel von den markeng menüs in Maya abschauen das ist wirklich hervorragend dort gelöst. Das mit den duotone icons könnte generell in der UI helfen solange es nicht zu viele Bereiche werden :D
- Horizon könnte man auch noch als shortcuts dort anlegen bzw vlt. könnte man sogar View presets speichern und die darüber gezielt immer wieder schnell anfahren (für streetview Wege alt relevant, das man immer +90° / -5° speichert)

# Sonstiges

- Eine history liste ähnliches zu Photoshop fehlt, wie schätzt du das technisch ein von der Umsetzbarkeit bzw hat das Konsequenzen an anderer stelle
- Die cmd + k suche finde ich super
- Ist der zen modus angelehnt an das was ich in Maya beschrieben habe mit Expert workspace? Falls ja dann passt das nicht so ganz, der blendet zu viel aus also aktuell ist das markeng Menu nicht in der lage alles zu lösen daher braucht man manche ui noch z.b das füllen von Masken, wobei ich mich hier korrigieren muss mit cmd + k und markig Menu kann man da durchaus einiges machen das ist schon sehr cool aber nicht sehr obvious für den user. Generell sollte man auch den shortcuts zeigen wie man da wieder raus kommt für Leute die versehentlich rein gehen.
- Der aktuelle Floating UI Ansatz wirkt modern und schlicht, ich bin mir noch nicht 100% was das ui design davon angeht. Generell bin ich offen für ui Konzepte dinge anders zu gestalten, zu erweitern, zusammenzufassen usw. UI UX sind mir sehr wichtig.
- In manchen szenarien ist mir aufgefallen das der Viewer sehr empfindlich auf drag changes reagierte aber in den meisten versuchen verhielt es sich normal. Es ist sehr wichtig das dass navigieren im viewport gut funktioniert das ist vermutlich der größte UX Anteil den wir haben in der app da der Viewer fast die ganze Sicht ist.
- Ah die export Option habe ich erst zum Schluss gefunden nur per hotkey aktuell minimalistisch aber erfüllt an sich die export zwecke für ein Einzelbild.
- Die vorher nachher Option also unbearbeitet / voll bearbeitet finde ich super, der hotkey müsste aber besser kommuniziert werden. (Alt hilft da auch das was ich oben schonmal meinte mit keys als keys stylen und anhand der Plattform erkennen was es für keys sind beim Mac z.b dieses cmd Zeichen?

Also im großen und ganzen finde ich das schon echt sehr gut gelungen! Super Arbeit! Ich habe versucht im Detail Feedback zu geben und auch einfach meine Gedanken aufzuschreiben. Ich denke das unklarste ist der Score der tools bzw Bearbeitungsfunktionen / Featuresets.
