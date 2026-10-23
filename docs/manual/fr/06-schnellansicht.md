# Coup d’œil et aperçu {#schnellansicht}

Appuyez sur **Espace** pour regarder un fichier sans l’ouvrir. Pour les fichiers de votre Mac, Manta utilise **Coup d’œil** d’Apple, comme le Finder ; pour les fichiers sur des serveurs et dans des archives, il dispose d’un **aperçu intégré**.

## Regarder rapidement des fichiers {#leertaste}

1. Sélectionnez un fichier.
2. Appuyez sur **Espace** (ou **⌘Y**, comme dans le Finder).
3. Utilisez **↑** et **↓** pour passer au fichier précédent ou suivant ; le panneau suit.
4. **Espace** ou **esc** ferme l’aperçu.

Si plusieurs fichiers sont sélectionnés, les flèches **‹** et **›** de Coup d’œil permettent de parcourir la sélection.

Le clavier reste dans Manta : vous pouvez continuer à vous déplacer dans la liste avec les touches fléchées pendant que le panneau Coup d’œil est ouvert.

![Coup d’œil au-dessus d’une sous-fenêtre](images/quick-look.png)

## Coup d’œil (macOS) {#quick-look}

Coup d’œil est le panneau d’aperçu d’Apple. Il affiche tout ce que le Finder affiche : PDF, images, vidéos, documents Office, fichiers texte et bien plus. Il fonctionne pour les fichiers et dossiers **locaux**.

## Aperçu intégré {#integrierte-vorschau}

Les fichiers sur des **serveurs** (SFTP, FTP, S3, conteneurs) et **dans des archives** s’ouvrent dans la fenêtre d’aperçu propre à Manta, car Coup d’œil d’Apple ne connaît que les fichiers locaux.

- L’aperçu intégré affiche les **fichiers texte** et les **images**.
- Pour les gros fichiers texte, il affiche le début (« Affichage des premiers … du fichier. »).
- Pour les autres types de fichiers (PDF ou vidéos, par exemple), il indique de quel fichier il s’agit et propose de l’ouvrir avec ↩. Manta le télécharge alors et l’ouvre avec l’app par défaut.
- Il n’affiche pas les dossiers ; appuyez sur ↩ pour en ouvrir un. (Coup d’œil d’Apple affiche les dossiers locaux.)
- Pour le fermer : **Espace** ou **⌘W** (c’est une fenêtre à part entière avec les boutons de fenêtre habituels).

![Aperçu intégré d’un fichier texte provenant d’un serveur](images/preview-builtin.png)

## Modifier le rôle de la touche Espace {#einstellung}

Dans **Réglages** → **Général** → **Aperçu** → **La barre d’espace ouvre**, choisissez :

- **Coup d’œil (macOS)** – par défaut.
- **Aperçu intégré** – y compris pour les fichiers locaux.

Les fichiers sur des serveurs et dans des archives utilisent toujours l’aperçu intégré.

## D’autres aperçus grâce aux plug-ins {#plugins}

- **Aperçu Markdown** (**⌥⌘M**) : affiche les fichiers `.md` joliment mis en forme dans un panneau latéral – voir [Aperçu Markdown](14-plugins.md#markdown).
- **Outils d’image** : affichent la taille, le profil colorimétrique et les données EXIF d’une image – voir [Outils d’image](14-plugins.md#bildwerkzeuge).

## Limites {#grenzen}

- Coup d’œil ne fonctionne qu’avec les fichiers locaux. Manta ne télécharge pas (encore) les fichiers de serveur pour les afficher dans Coup d’œil.
- L’aperçu intégré n’affiche que du texte et des images.
