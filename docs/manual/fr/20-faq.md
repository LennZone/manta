# Questions fréquentes {#faq}

Réponses courtes aux questions les plus fréquentes. Les chapitres en lien donnent les détails.

## Général

### Manta remplace-t-il le Finder ? {#finder-ersetzen}

Pas entièrement. Vous pouvez faire de Manta votre gestionnaire de fichiers par défaut ; « Afficher dans le Finder » et (jusqu’à macOS 15) les dossiers ouverts par d’autres apps s’ouvrent alors dans Manta. Mais le Bureau, l’icône du Finder dans le Dock et les doubles-clics dans les fenêtres du Finder restent toujours dans le Finder – macOS ne permet à aucune app de les reprendre. Voir [Gestionnaire de fichiers par défaut](01-erste-schritte.md#standard-dateimanager).

### Manta est-il payant ou collecte-t-il des données ? {#daten}

Manta ne collecte aucune donnée d’utilisation. Il ne se connecte à Internet que pour vos propres connexions et pour la recherche de mises à jour, qui n’envoie que votre adresse IP et la version de Manta. Voir [Confidentialité](17-updates.md#datenschutz).

Manta est gratuit et open source sous licence MIT : vous pouvez l’utiliser, le modifier et le partager librement. Le code source se trouve sur [GitHub](https://github.com/web-workers-com/manta) (dans l’app, via **Aide** → **Code source sur GitHub…**) et les contributions sont les bienvenues. Actualités, aide et contact se trouvent sur le site web [manta-filemanager.com](https://manta-filemanager.com/) – dans l’app, via le menu **Aide** → **Site web et assistance Manta…** et dans les Réglages, sous [À propos de Manta](16-einstellungen.md#ueber).

Manta repose sur des composants open source. Leurs mentions de licence se trouvent sous **Aide** → **Licences de tiers** ou [ici](../../third-party-licenses.md).

### Manta est-il disponible en français ? {#sprache}

Oui. **Réglages** → **Général** → **Langue** : **Système** (par défaut, suit vos langues préférées de macOS) ou l’une des sept langues de l’app : **Deutsch**, **English**, **简体中文**, **Español**, **Français**, **Italiano**, **Русский**. Le changement s’applique immédiatement. Le manuel (**⌘?**) suit aussi la langue de l’app ; s’il n’existe pas dans cette langue, Manta affiche la version anglaise.

### Combien de sous-fenêtres puis-je ouvrir ? {#bereiche}

Autant que votre fenêtre peut en contenir – chacune a besoin d’environ 280 points de largeur. Au moins une reste toujours ouverte. Voir [Sous-fenêtres et onglets](02-bereiche-tabs.md).

## Fichiers

### Où est la corbeille ? Puis-je récupérer des fichiers supprimés ? {#papierkorb}

Oui, pour les éléments de votre Mac : **⌘⌫** les place dans la corbeille de macOS, comme le Finder, et la commande **Remettre** du Finder les restaure. **⌥⌘⌫** (**Supprimer immédiatement…**) supprime définitivement après confirmation. Les serveurs, S3 et les conteneurs n’ont pas de corbeille : la suppression y est toujours définitive, et Manta demande d’abord confirmation. Voir [Supprimer](05-dateien.md#loeschen).

### Le glisser-déposer copie-t-il ou déplace-t-il ? {#ziehen}

Manta vous demande, avec **Copier** présélectionné – même sur le même volume, où le Finder déplacerait. Ainsi, rien ne disparaît par mégarde de son dossier. Maintenez **⌘** en relâchant pour présélectionner **Déplacer** ; **⌥** présélectionne **Copier**. La pastille près du pointeur vous indique à l’avance ce qui est présélectionné. Dans les [Réglages](16-einstellungen.md#kopieren-bewegen), vous pouvez choisir à la place « Toujours copier » ou « Toujours déplacer » (sans zone de dialogue). Le dépôt sur la corbeille place toujours les éléments dans la corbeille. Voir [Glisser-déposer](05-dateien.md#drag-drop).

### J’ai ouvert un fichier depuis un serveur et l’ai modifié – où est ma modification ? {#server-datei}

Dans une copie temporaire sur votre Mac. Manta ne téléverse pas automatiquement les fichiers modifiés. Copiez le fichier dans un dossier local, modifiez-le à cet endroit et recopiez-le avec **F5**. Voir [Navigation](04-navigation.md#oeffnen).

### Pourquoi ne vois-je pas mes fichiers `.env` ou `.git` ? {#unsichtbar}

Les fichiers dont le nom commence par un point sont masqués. Appuyez sur **⇧⌘.**. Voir [Fichiers masqués](04-navigation.md#unsichtbare-dateien).

### Pourquoi un dossier ne se met-il pas à jour lorsqu’une autre app ajoute des fichiers ? {#aktualisieren}

Appuyez sur **⌘R**. Manta relit les dossiers après ses propres actions, mais pas lorsque d’autres apps modifient quelque chose.

### Puis-je modifier des fichiers à l’intérieur d’archives ZIP ? {#archiv-aendern}

Non, les archives sont en lecture seule. Extrayez-les, modifiez les fichiers et compressez-les de nouveau (**⌥⌘C**). Voir [Archives](10-archive.md).

### Qu’advient-il d’une copie si le Wi-Fi est coupé ou si je quitte Manta ? {#unterbrochen}

Rien n’est perdu. Lorsque le réseau tombe, le transfert attend et reprend au même point dès que la connexion est rétablie. Lorsque vous quittez Manta (ou s’il plante), il demande « Reprendre les transferts interrompus ? » au lancement suivant. Voir [Suspendre et reprendre les transferts](05-dateien.md#pausieren).

### Que sont les fichiers `.NOM.manta-part` ? {#manta-part}

Des fichiers partiels masqués dans lesquels Manta écrit pendant la copie. Seul le fichier terminé reçoit son vrai nom. Voir [Fichiers partiels](05-dateien.md#teildateien).

## Connexions et cloud

### Mes mots de passe de serveur sont-ils stockés en toute sécurité ? {#passwoerter}

Oui. Avec **Mémoriser le mot de passe**, Manta stocke les mots de passe, phrases de passe et clés secrètes S3 dans le trousseau macOS, et non dans un fichier. Les mots de passe que d’anciennes versions avaient enregistrés sans chiffrement y sont déplacés au lancement. Sans **Mémoriser le mot de passe**, Manta les demande à chaque fois. SFTP avec ssh-agent ne nécessite aucun mot de passe enregistré. Voir [Connexions](08-verbindungen.md#favoriten).

### Puis-je me connecter directement à Google Drive ? {#google}

Non. Installez **Google Drive pour ordinateur** et connectez-vous dans cette app ; Manta affiche alors le dossier Google Drive. Voir [Stockage cloud](09-cloud.md#google-drive).

### Manta prend-il en charge WebDAV, SMB ou Dropbox sans l’app ? {#protokolle}

Manta se connecte directement à SFTP, FTP, FTPS, S3 et aux conteneurs. Montez les partages SMB et WebDAV dans le Finder (**Aller** → **Se connecter au serveur**) ; ils apparaissent alors sous **Appareils**. Manta affiche Dropbox, OneDrive et consorts via les apps des fournisseurs.

### Pourquoi y a-t-il un bouclier d’avertissement sur la pastille FTP ? {#ftp-warnung}

Votre réseau bloque les connexions de données FTP chiffrées. Manta transfère donc les fichiers sans chiffrement ; la connexion reste chiffrée. Voir [Canal de données](08-verbindungen.md#datenkanal).

### Manta peut-il démarrer ou arrêter des conteneurs ? {#container}

Non. Manta se contente d’afficher et de modifier les fichiers dans les conteneurs. Démarrez et arrêtez les conteneurs avec Docker Desktop ou dans le terminal.

## Étendre Manta

### Puis-je ajouter mes propres commandes sans écrire de plug-in ? {#eigene-befehle}

Oui, avec les **Actions rapides** : une entrée avec une commande shell dans `actions.json` suffit. Voir [Actions rapides](14-plugins.md#actions-json).

### Les plug-ins sont-ils sûrs ? {#plugins-sicher}

Les plug-ins intégrés font partie de Manta. Les plug-ins externes s’exécutent avec les autorisations de votre utilisateur et sans bac à sable – n’installez que ce en quoi vous avez confiance. Voir [Vos propres plug-ins](15-eigene-plugins.md#sicherheit).
