# Stockage cloud {#cloud}

Manta affiche iCloud Drive, Google Drive, OneDrive, Dropbox et les autres stockages cloud que votre Mac synchronise déjà. Vous voyez quels fichiers se trouvent uniquement dans le cloud, vous pouvez les télécharger ou supprimer à nouveau le téléchargement local.

![Section Cloud de la barre latérale et pastilles de synchronisation dans la liste](images/cloud-sidebar-badges.png)

## Comment Manta accède au stockage cloud {#prinzip}

Manta ne se connecte **pas** lui-même à Google, Microsoft ou Dropbox. Il utilise les dossiers que les apps officielles des fournisseurs créent sur votre Mac :

- **iCloud Drive** est intégré à macOS.
- **Google Drive**, **OneDrive**, **Dropbox**, **Box** et d’autres créent leurs dossiers dans `~/Library/CloudStorage` une fois que vous avez installé leur app et que vous vous y êtes connecté.

Pour Manta, ce sont des dossiers ordinaires de votre Mac. Quoi que vous fassiez dans Manta – copier, renommer, supprimer –, l’app du fournisseur synchronise ensuite avec le cloud.

## Configurer Google Drive {#google-drive}

1. Téléchargez **Google Drive pour ordinateur** sur le site de Google et installez-le.
2. Connectez-vous à l’app Google Drive avec votre compte Google.
3. **Google Drive** apparaît de lui-même sous **Cloud** dans la barre latérale – Manta repère les nouveaux dossiers dans `~/Library/CloudStorage`. Si ce n’est pas le cas, cliquez sur la flèche circulaire à côté de **Cloud**.

Configurez OneDrive et Dropbox de la même façon : installez l’app du fournisseur, connectez-vous, c’est tout.

> [!NOTE]
> Manta n’a **pas sa propre connexion Google**. Sans Google Drive pour ordinateur, vous ne verrez pas Google Drive dans Manta.

Tant qu’il n’y a pas de dossier Google Drive, **Cloud** affiche une ligne **Connecter Google Drive…**. Cliquez dessus pour voir les étapes de configuration et le lien **Obtenir Google Drive pour ordinateur** (<https://www.google.com/drive/download/>). **Ne plus afficher** masque l’indication ; **Réglages › Général › Cloud** la fait réapparaître.

## La section Cloud de la barre latérale {#seitenleiste}

La section **Cloud** se trouve entre **Emplacements** et **Appareils**. Chaque stockage cloud trouvé a sa propre ligne avec l’icône du fournisseur. Si un fournisseur apparaît plusieurs fois (deux comptes Google, le OneDrive de votre entreprise et des bibliothèques partagées), le compte suit en gris ; sinon, l’infobulle l’affiche avec le chemin :

- iCloud Drive (toujours en premier lorsqu’il est activé)
- Google Drive (avec plusieurs comptes : Google Drive *nom@gmail.com*)
- OneDrive *Personnel* ou OneDrive *nom de votre entreprise*, OneDrive *Bibliothèques partagées*
- Dropbox, Box et d’autres (Nextcloud, Proton Drive, Synology Drive, par exemple)

Un clic ouvre le stockage dans la sous-fenêtre active.

## iCloud Drive comme dans le Finder {#icloud-apps}

Dans Manta, iCloud Drive se présente comme dans le Finder : le dossier de plus haut niveau affiche vos dossiers et fichiers **et** les dossiers de vos apps – par exemple **Obsidian**, **Pages**, **Numbers** ou **Raccourcis**. Manta affiche les mêmes dossiers d’apps que le Finder (les apps qui partagent leurs documents dans iCloud Drive et en ont déjà). Chaque dossier d’app affiche l’icône de son app si celle-ci est installée sur votre Mac (réglage **Icônes de fichiers de macOS**).

La barre du chemin d’accès commence par la pastille **iCloud Drive** ; dans un dossier d’app, le nom de l’app suit, par exemple **iCloud Drive › Obsidian › Vault**. Les noms de dossier sous lesquels macOS range tout cela sur le disque (`~/Library/Mobile Documents/com~apple~CloudDocs`, `iCloud~md~obsidian/Documents`) n’apparaissent que lorsque vous modifiez le chemin.

Un dossier d’app appartient à son app : vous ne pouvez pas le renommer, le déplacer ni le supprimer – Manta vous le signale si vous essayez. Son contenu, en revanche, se modifie normalement.

## Espace libre {#freier-speicher}

En bas, dans la barre d’état, Manta indique après le nombre d’éléments l’espace encore libre, par exemple « 12 éléments · 18 GB disponibles ». L’infobulle affiche aussi la taille totale, si elle est connue.

| Où | Ce que signifie « disponibles » |
|---|---|
| Dossiers sur votre Mac | espace libre du disque – comme dans le Finder, donc y compris l’espace que macOS libère au besoin |
| iCloud Drive | ce qui reste libre dans votre stockage iCloud |
| Google Drive (connexion propre) | ce qui reste libre dans votre stockage Google ; rien dans les Drive partagés |
| SFTP, SMB | espace libre sur le serveur |
| WebDAV (par exemple Nextcloud) | votre quota sur le serveur, s’il en indique un |

Google Drive pour ordinateur, OneDrive et Dropbox stockent leurs fichiers sur votre Mac ; Manta y affiche l’espace libre du disque. S3, FTP et les conteneurs n’indiquent pas d’espace libre – l’information est alors absente. Après une copie, un déplacement ou une suppression, la valeur est relue.


## Signification des pastilles {#abzeichen}

Dans les dossiers cloud, Manta indique l’état de synchronisation : dans la liste sous forme d’icône à l’extrémité droite de la colonne Nom, dans la présentation Icônes sous forme de pastille ronde en haut à droite de l’icône. L’infobulle le formule en toutes lettres (par exemple « Uniquement dans le cloud – non téléchargé »).

| Pastille | Signification | Actions possibles |
|---|---|---|
| (aucune) | Téléchargé et à jour | Supprimer le téléchargement |
| Nuage avec une flèche vers le bas | Uniquement dans le cloud, pas sur ce Mac | Télécharger maintenant |
| Nuage avec un point d’exclamation | Téléchargé, mais une version plus récente existe dans le cloud | Télécharger maintenant, Supprimer le téléchargement |
| Cercle avec une flèche vers le bas | Téléchargement en cours | – |
| Nuage avec une flèche vers le haut (bleu, pulsant) | Téléversement en cours ou en attente – par exemple juste après une copie dans Google Drive | – |
| Point d’exclamation rouge dans un cercle | Erreur de téléversement : le fournisseur n’a pas pu téléverser l’élément ; l’infobulle en donne la raison (par exemple « Erreur de téléversement : stockage plein ») | – |

Après une copie dans un dossier cloud, l’app du fournisseur téléverse en arrière-plan. Pendant ce temps, la barre d’état indique aussi « Google Drive téléverse 2 éléments » (en rouge en cas d’erreur). iCloud Drive peut aussi signaler la progression (« Téléversement… 42 % » dans l’infobulle) ; Google Drive, OneDrive et Dropbox n’indiquent que « téléversement en cours ». Les fichiers que les fournisseurs ne téléversent jamais (`.DS_Store`, fichiers de verrouillage Office `~$…`) n’ont pas de pastille.

La lecture de l’état ne télécharge rien : vous pouvez donc parcourir de grands dossiers cloud sans que votre Mac récupère tout.

Manta lit l’état à partir des attributs des fichiers (l’indicateur `SF_DATALESS` et les valeurs `NSURLUbiquitousItem…` de macOS, que signalent iCloud Drive et les apps File Provider) et l’actualise toutes les quelques secondes tant que le dossier est visible.

## Télécharger des fichiers depuis le cloud {#jetzt-laden}

1. Sélectionnez des fichiers ou des dossiers qui se trouvent uniquement dans le cloud.
2. Clic droit → **Télécharger maintenant**.

Les dossiers sont téléchargés avec tout leur contenu. La pastille passe à « téléchargement en cours » et disparaît une fois que tout est là.

Si vous ouvrez ou copiez un fichier qui se trouve uniquement dans le cloud, macOS le télécharge de toute façon automatiquement.

## Libérer de l’espace {#download-entfernen}

1. Sélectionnez des fichiers ou des dossiers téléchargés.
2. Clic droit → **Supprimer le téléchargement**.

Les fichiers restent dans le cloud et visibles dans Manta, mais n’occupent plus d’espace sur votre Mac.

> [!IMPORTANT]
> **Supprimer le téléchargement** fonctionne de manière fiable pour **iCloud Drive**. Pour Google Drive, OneDrive, Dropbox et les autres, c’est l’app du fournisseur qui décide de l’autoriser ou non. En cas de refus, Manta vous le signale. Utilisez alors le **Finder** : clic droit → **Supprimer le téléchargement** – le Finder y est autorisé, les autres apps non.

## Google Drive pour les experts : votre propre client OAuth {#experte}

Manta contient aussi sa propre connexion Google Drive (Drive API), qui est **masquée** : Manta ne fournit pas de client OAuth pour celle-ci. Uniquement si vous ne pouvez pas utiliser Google Drive pour ordinateur et disposez de votre propre client :

1. Dans la Google Cloud Console, créez un client OAuth de type **Desktop app** et activez la Google Drive API.
2. Ouvrez **Réglages › Général › Cloud › Expert : votre propre client OAuth Google**, saisissez l’ID client et le secret client, puis cliquez sur **Enregistrer**.
3. La zone de dialogue de connexion propose désormais un type **Google Drive**. Si vous laissez **Compte** vide, Manta ouvre le navigateur pour vous identifier lors de la connexion.

**Supprimer** retire à nouveau Google Drive de la zone de dialogue de connexion. Le client est conservé dans `~/Library/Application Support/Manta/gdrive-client.json`.

## Supprimer dans les dossiers cloud {#loeschen}

> [!IMPORTANT]
> Les dossiers cloud sont des dossiers de votre Mac. **⌘⌫** place les éléments qu’ils contiennent dans la **Corbeille**, comme partout ailleurs (voir [Supprimer](05-dateien.md#loeschen) et [Corbeille](07-favoriten-seitenleiste.md#papierkorb)) ; **⌥⌘⌫** supprime définitivement après confirmation. L’app du fournisseur synchronise ensuite la suppression **dans le cloud et sur tous vos appareils**. Selon le fournisseur, le fichier reste encore quelque temps dans la corbeille de son site web.

Si vous voulez seulement gagner de l’espace, utilisez **Supprimer le téléchargement** au lieu de supprimer.

## Limites {#grenzen}

- Manta n’affiche que les stockages cloud que votre Mac synchronise déjà. Il ne fonctionne pas sans l’app du fournisseur.
- **Supprimer le téléchargement** pour les fournisseurs tiers uniquement si leur app le permet (sinon dans le Finder).
- La vitesse à laquelle les modifications parviennent au cloud dépend de l’app du fournisseur, pas de Manta.
