# Dépannage {#fehlerbehebung}

Solutions aux problèmes courants, et emplacement où Manta consigne les erreurs. Si vous signalez un bug, le fichier journal est d’une grande aide.

## Le fichier journal {#protokoll}

Manta consigne les erreurs et les événements importants dans :

`~/Library/Logs/Manta/manta.log`

Pour le trouver :

1. Ouvrez la palette de commandes avec **⇧⌘P**.
2. Choisissez **Afficher le fichier journal dans le Finder**. Le Finder ouvre le dossier des journaux (`~/Library/Logs/Manta`) avec le fichier `manta.log` – dans le Finder lui-même, même si Manta est votre gestionnaire de fichiers par défaut.

Chaque message d’erreur que Manta vous affiche y figure, avec l’erreur technique, le chemin et l’emplacement (local, serveur…). Le fichier est renouvelé à 5 Mo ; une copie plus ancienne est conservée.

> [!TIP]
> Pour des journaux plus détaillés, lancez Manta depuis Terminal avec `MANTA_LOG=debug /Applications/Manta.app/Contents/MacOS/manta`.

> [!NOTE]
> Le fichier journal peut contenir des noms de fichiers et de serveurs. Vérifiez-le avant de le partager. Les mots de passe, phrases de passe et clés secrètes n’y figurent pas – y compris pour SFTP, FTP et S3.

Les plug-ins ont leurs propres journaux : **Réglages** → **Plug-ins** → **Afficher le journal** sur la carte du plug-in.

## Manta ne s’ouvre pas (Gatekeeper) {#gatekeeper}

**Problème :** au premier lancement, macOS affiche « “Manta” n’a pas été ouvert » ou « … ne peut pas être ouvert, car Apple ne peut pas vérifier qu’il ne contient pas de logiciel malveillant ».

**Cause :** cette copie n’est pas notariée par Apple. macOS bloque ces apps au premier lancement.

**Solution :**

1. Cliquez sur **Terminé**.
2. Ouvrez **Réglages Système** → **Confidentialité et sécurité**.
3. En bas, cliquez sur **Ouvrir quand même** à côté de Manta et confirmez.

Avant macOS 15 : clic droit (ou Contrôle-clic) sur Manta dans le dossier **Applications** → **Ouvrir** → **Ouvrir**.

## « macOS a bloqué l’accès » {#berechtigungen}

**Problème :** un dossier ne s’ouvre pas, ou Manta indique « macOS a bloqué l’accès à « … » (opération non autorisée) ».

**Cause :** vous n’avez pas autorisé Manta à accéder à ce dossier (ou vous avez cliqué sur « Refuser » par erreur), ou le dossier nécessite l’Accès complet au disque.

**Solution :**

1. Ouvrez **Réglages Système** → **Confidentialité et sécurité** → **Fichiers et dossiers**.
2. Trouvez **Manta** et activez le dossier (Bureau, Documents, Téléchargements, volumes amovibles, volumes réseau).
3. Pour des dossiers comme `~/Library/Mail` : **Accès complet au disque** → activez **Manta**.
4. Quittez Manta et relancez-le.

Si Manta ne figure pas dans la liste, macOS n’a pas encore posé la question. Ouvrez une fois le dossier dans Manta et la question apparaît.

« Vous n’avez pas l’autorisation d’ouvrir ce dossier » est un autre problème : les autorisations Unix habituelles manquent. Demandez au propriétaire du dossier ou modifiez les autorisations dans le Finder (**Lire les informations**).

## Fonctions qui pilotent le Finder ou Terminal {#automation}

**Problème :** « Afficher dans le Finder », « Ouvrir dans Terminal.app » ou le placement des doublons dans la corbeille ne fonctionne pas.

**Solution :** **Réglages Système** → **Confidentialité et sécurité** → **Automatisation** → sous **Manta**, activez **Finder** et **Terminal**.

## FTP : les dossiers ne se chargent pas, erreurs « TLS » {#ftp-datenkanal}

**Problème :** la connexion à un serveur FTPS fonctionne, mais les dossiers restent vides ou se chargent indéfiniment, ou Manta indique « le réseau bloque les connexions de données FTP chiffrées ».

**Cause :** certains réseaux et pare-feu d’entreprise réinitialisent les connexions de données FTP chiffrées.

**Solution :**

1. Clic droit sur le favori → **Modifier la connexion…** (ou **⌘K** pour une nouvelle connexion).
2. Réglez **Canal de données** sur **Automatique**.
3. Reconnectez-vous.

Manta transmet alors les listes et les fichiers sans chiffrement ; la connexion reste chiffrée. Un bouclier d’avertissement sur la pastille d’emplacement l’indique.

> [!WARNING]
> Ce n’est pas une bonne solution pour des données confidentielles. Utilisez plutôt SFTP, un autre réseau ou un VPN, et réglez le canal de données sur **Obligatoire**.

Autres conseils pour FTP :

- « Impossible d’établir une connexion sécurisée » : le serveur ne prend pas en charge FTPS sur ce port (essayez **FTP** si c’est acceptable sur votre réseau), ou son certificat n’est pas approuvé.
- Manta ne prend pas en charge le FTPS implicite (port 990).

## Un conteneur sans shell {#container-ohne-shell}

**Problème :** un conteneur s’ouvre en lecture seule, ou Manta indique « Le conteneur n’a pas de shell ».

**Cause :** Manta a besoin de `sh` dans le conteneur pour lister et modifier les fichiers. Les images distroless et scratch n’ont pas de shell.

**Solution :**

- La lecture et la copie vers l’extérieur (**F5**, glisser-déposer) fonctionnent toujours – via `docker cp`.
- Pour apporter des modifications, il vous faut une image avec un shell (une variante de débogage de l’image, par exemple).
- Les pods Kubernetes ont besoin de `sh`, même pour la lecture.
- Les conteneurs arrêtés sont aussi en lecture seule : démarrez-les pour apporter des modifications.

## Docker ou Kubernetes n’apparaissent pas {#container-fehlen}

- **« Docker n’est pas en cours d’exécution »** : lancez Docker Desktop.
- **« Docker n’est pas installé »** : installez Docker Desktop, ou vérifiez que la commande `docker` se trouve dans votre PATH. Manta cherche aussi dans `/opt/homebrew/bin`, `/usr/local/bin` et dans l’app Docker Desktop.
- **« kubectl n’est pas installé »** : `brew install kubectl`.
- **« Le cluster Kubernetes est inaccessible »** : vérifiez le contexte et votre VPN, ou démarrez votre cluster local.

## Connexions aux serveurs {#server}

| Message | Solution |
|---|---|
| « La clé du serveur a changé » | La clé ne correspond pas à `~/.ssh/known_hosts` ; Manta ne se connecte pas (attaque de l’homme du milieu possible). Demandez l’empreinte à l’administrateur. Si le serveur a réellement été réinstallé, supprimez l’ancienne entrée avec la commande indiquée dans la zone de dialogue (`ssh-keygen -R host`). |
| « Serveur inconnu » | Première connexion à ce serveur. Comparez l’empreinte et cliquez sur **Se connecter et mémoriser**, voir [Connexions](08-verbindungen.md). |
| « Le mot de passe enregistré est introuvable dans le trousseau » | L’élément a été supprimé, par exemple dans Trousseaux d’accès. Saisissez le mot de passe et laissez **Mémoriser le mot de passe** activé. |
| « Aucune clé utilisable dans ssh-agent » | `ssh-add ~/.ssh/id_ed25519` |
| « L’horloge de votre Mac est déréglée » | **Réglages Système** → **Général** → **Date et heure** → réglage automatique. |
| « Le bucket se trouve dans une autre région » | Dans la zone de dialogue, réglez la région sur celle indiquée. |
| « La connexion au serveur a été perdue » | Vérifiez votre réseau, puis cliquez sur **Réessayer**. |

## Un transfert n’avance pas {#uebertragungen}

| Ce que vous voyez | Solution |
|---|---|
| « En attente de connexion… (tentative 5, suivante dans 16 s) » | Le réseau ou le serveur a disparu. Manta continue d’essayer et reprend au même point. Vérifiez le Wi-Fi/VPN ; **Réessayer maintenant** lance immédiatement la tentative suivante. |
| **Aucune connexion à Internet** en haut de **Transferts** | Votre Mac est hors ligne. Dès qu’il est de nouveau en ligne, les transferts reprennent d’eux-mêmes. |
| « Nécessite une connexion à… » | Après un redémarrage, la connexion au serveur manque. Cliquez sur **Reconnecter** et saisissez le mot de passe. |
| « Suspendu · … » | Vous avez suspendu la tâche (ou **Tout suspendre** l’a fait). Cliquez sur **Reprendre**. |
| Fichiers masqués `.NOM.manta-part` dans la cible | Fichiers partiels d’une tâche interrompue. Laissez-les tant que la tâche doit être reprise ; sinon, vous pouvez les supprimer. |

Voir [Suspendre et reprendre les transferts](05-dateien.md#pausieren).

## Une mise à jour échoue {#updates}

Voir [Mises à jour](17-updates.md#blockiert). Le plus souvent, Manta n’est pas exécuté depuis le dossier **Applications**.

## Cloud : « Supprimer le téléchargement » ne fonctionne pas {#cloud}

Pour Google Drive, OneDrive, Dropbox et les autres, c’est l’app du fournisseur qui décide. Utilisez le Finder : clic droit → **Supprimer le téléchargement**. Voir [Stockage cloud](09-cloud.md#download-entfernen).

## Un dossier affiche un contenu obsolète {#aktualisieren}

Appuyez sur **⌘R**. Manta ne voit les modifications effectuées par d’autres apps ou par le serveur qu’après une actualisation.

## Réinitialiser Manta {#zuruecksetzen}

En dernier recours :

1. Quittez Manta.
2. Renommez `~/Library/Application Support/Manta` (en `Manta ancien`, par exemple).
3. Lancez Manta. Il démarre comme s’il s’agissait du premier lancement.

Vos favoris et vos actions rapides se trouvent toujours dans le dossier renommé.
