// Macro (social) helpers: a curated library of common EQL macros, the authoritative slash-command
// set for typo validation, and the substitution tokens the game understands. The command set is
// embedded (not read from the client at runtime) so validation works on any install.
//
// Sources: the client's own `slash_commands_EQLegends.md` (372+ commands from eqgame.exe) and
// classic-EQ social conventions. `/pause N` = N TENTHS of a second (15 = 1.5s). A line without a
// leading "/" is spoken to your current chat channel (usually /say), which is valid — not an error.

export interface LibraryMacro {
  category: string;
  name: string;   // the button label (keep short; the in-game button shows only a few chars)
  color: number;  // 0-15 palette index
  lines: string[];
  note?: string;
}

/** Every slash command the EQL client knows (lowercased, no leading slash). GM/guide commands are
 *  included — they're real client commands, just server-gated — so they don't flag as typos. */
export const KNOWN_COMMANDS: Set<string> = new Set(
  (
    "achievements aclearcompare acompare adventure advloot afk afp aggressive aggrolock aggrometer " +
    "agree airlute alarm all alternateadv amaze announce anonymous apologize applaud assist attack " +
    "auction autobank autoconsent autofire autoinventory autojoin automergeinventory autoskill " +
    "autosplit bandolier bazaar becomenpc bird bite bleed blink blockspell blush bodytint boggle " +
    "bonk book bored bounce bow brb broadcast bugreport burp buyer bye cackle calendar calm camp " +
    "cast changefamiliarname changemercname changename changepetname channel charinfo chat " +
    "chatfontsize cheer chuckle claim clap clearallchat clearchat clearmarks clickthrough combatmusic " +
    "comfort congratulate consent consider copylayout corpse corpsedrag corpsedrop cough cringe cry " +
    "curious dance decaycorpse default delcorpse delegategmarknpc deny destroyitem disband discipline " +
    "discord dismount doability drool duck duel dynamiclights dzaddplayer dzhelp dzlisttimers " +
    "dzmakeleader dzplayerlist dzquit dzremoveplayer dzswapplayer emote emoteworld emotezone event " +
    "exit extralife eye facepalm facepick faction faint familiar fastdrop feedback fidget fight " +
    "filter find finditem findpc finger fistbump flex flipoff follow fontface freakout friends frown " +
    "fsay fullscreen gasp gems gesture getguildmotd giggle glare gmarknpc goto grin groan groupleader " +
    "grouproles grovel growl gsay guidehelp guildcreate guilddelete guilddemote guildinvite " +
    "guildleader guildmotd guildpromote guildremove guildsay guildstatus hail happy hate height help " +
    "hidecorpses hideme hidemodels hotbutton hug hungry ignore indicator inspect inspectbuffs " +
    "introduce invite itemoverflow join keys kick kickplayers kill kiss kneel language lastname laugh " +
    "leave leaveall leaverealestate lfgroup lfguild list loadskin location lockconfirm log loginterval " +
    "loot lootnodrop lost makeleader map marketplace marknpc massage maybe mcicontrol melody memspellset " +
    "memspellslot mercassist merclog mercresetaas mercswitch mercwindows mixahead moan motd mourn msg " +
    "netstats nod note nudge offlinemode ooc open outputfile overseer overseermassconvert panic " +
    "particledensity pat pause peer pet petition pickzone pickzonefewestplayers pie pizza played plead " +
    "point poke ponder private purr puzzle queuemelody quit racechange raise random ready realestate " +
    "renameguild reply report reservename reveal rewards rewind roar rofl rps rsay rtarget rude runmode " +
    "safelock salute say searchcorpse selfkill send server servers servertransfer setstartcity sheathe " +
    "shield shieldgroup shiver shout shownames shownpcnames showspelleffects shrug sigh sit smack smile " +
    "smirk snarl snicker spellscribe split stance stand stare stopcast stopdisc stopsocial stopsong " +
    "stoptracking storage summon summoncorpse surname swarm system tap target targetgroupbuff " +
    "targetoftarget taskaddplayer taskhelp taskmakeleader taskoverlay taskplayerlist taskquit " +
    "taskremoveplayer tasktimers tease tell testcopy tgb thank thirsty time timer toggleinspect " +
    "toggletell trackfilter trackpets trackplayers tracksort trader tribute trophy ttell uptime url " +
    "useitem usercolor usetarget usurp veto vgroup viewport vplay vraid vtell wave waypoint wedding " +
    "welcome whine whistle who whotarget www xtarget yawn yell yes zone zt1"
  ).split(/\s+/)
);

/** Substitution tokens the game replaces at run time (classic EQ). Shown as editor hints. */
export const MACRO_TOKENS: { token: string; meaning: string }[] = [
  { token: "%t", meaning: "your current target's name" },
  { token: "%s", meaning: "target's subject pronoun (he/she/it)" },
  { token: "%o", meaning: "target's object pronoun (him/her/it)" },
];

/** classic-EQ command-line length cap; the game truncates a social line past this */
export const MAX_LINE_LEN = 255;

/** The leading /command token of a line, lowercased (null if the line isn't a command). */
export function commandOf(line: string): string | null {
  const m = line.trim().match(/^\/([a-z][a-z0-9]*)/i);
  return m ? m[1].toLowerCase() : null;
}

/** If a line starts with "/" but the command isn't a real EQL command, return the bad token
 *  (likely a typo); otherwise null. Plain-text lines (no slash = chat) never flag. */
export function unknownCommand(line: string): string | null {
  const t = line.trim();
  if (!t.startsWith("/")) return null;
  const cmd = commandOf(t);
  if (cmd && KNOWN_COMMANDS.has(cmd)) return null;
  return cmd ?? t.split(/\s+/)[0].replace(/^\//, "");
}

// ------------------------------------------------------------------ the curated library
// Colors: 1 blue (cast), 2 red (combat), 3 green (pet), 4 gold (utility), 5 magenta (bard),
// 8 lavender (travel/group), 0 white (social/RP). See MacrosTab's swatch palette.
export const MACRO_LIBRARY: LibraryMacro[] = [
  // ---- Combat ----
  { category: "Combat", name: "Assist+Atk", color: 2, lines: ["/assist", "/attack on"], note: "assist your target, then swing" },
  { category: "Combat", name: "Attack Off", color: 2, lines: ["/attack off"], note: "stop auto-attack (disengage)" },
  { category: "Combat", name: "Consider", color: 2, lines: ["/consider"], note: "gauge the target's difficulty" },
  { category: "Combat", name: "Taunt", color: 2, lines: ["/doability Taunt"], note: "warrior/knight taunt ability" },
  { category: "Combat", name: "Pull %t", color: 2, lines: ["/say Pulling %t", "/assist"], note: "call the pull, then assist" },

  // ---- Casting / spells ----
  { category: "Casting", name: "Cast 1", color: 1, lines: ["/cast 1"], note: "cast the spell in gem slot 1 (change the number for other gems)" },
  { category: "Casting", name: "Cast 2", color: 1, lines: ["/cast 2"] },
  { category: "Casting", name: "Cast 3", color: 1, lines: ["/cast 3"] },
  { category: "Casting", name: "Stop Cast", color: 1, lines: ["/stopcast"], note: "interrupt your current cast" },
  { category: "Casting", name: "Nuke %t", color: 1, lines: ["/target %t", "/cast 1"], note: "make sure your target is set, then nuke" },

  // ---- Pet ----
  { category: "Pet", name: "Pet Atk", color: 3, lines: ["/pet attack"], note: "send pet at your current target" },
  { category: "Pet", name: "Pet Back", color: 3, lines: ["/pet back off"], note: "call the pet off" },
  { category: "Pet", name: "Pet Guard", color: 3, lines: ["/pet guard here"], note: "hold position" },
  { category: "Pet", name: "Pet Follow", color: 3, lines: ["/pet follow"] },
  { category: "Pet", name: "Pet Taunt", color: 3, lines: ["/pet taunt on"], note: "tank pets: taunt on; use 'off' for DPS pets" },
  { category: "Pet", name: "Pet Sit", color: 3, lines: ["/pet sit"], note: "sit the pet to regen (where supported)" },
  { category: "Pet", name: "Pet GTFO", color: 3, lines: ["/pet get lost"], note: "dismiss the pet" },

  // ---- Bard ----
  { category: "Bard", name: "Twist 3", color: 5, lines: ["/melody 1 2 3"], note: "twist songs in gems 1-3 (bard)" },
  { category: "Bard", name: "Twist 4", color: 5, lines: ["/melody 1 2 3 4"] },
  { category: "Bard", name: "Stop Song", color: 5, lines: ["/stopsong"] },

  // ---- Class abilities ----
  { category: "Abilities", name: "Forage", color: 4, lines: ["/doability Forage", "/autoinventory"], note: "forage, then auto-stow the result" },
  { category: "Abilities", name: "Sneak+Hide", color: 4, lines: ["/doability Sneak", "/pause 10", "/doability Hide"], note: "rogue: sneak then hide (pause = 1s)" },
  { category: "Abilities", name: "Mend", color: 4, lines: ["/doability Mend"], note: "monk self-heal" },
  { category: "Abilities", name: "Feign", color: 4, lines: ["/doability \"Feign Death\""], note: "monk feign death" },

  // ---- Utility ----
  { category: "Utility", name: "Camp", color: 4, lines: ["/camp"], note: "start the camp-out timer" },
  { category: "Utility", name: "Sit", color: 4, lines: ["/sit"] },
  { category: "Utility", name: "AutoInv", color: 4, lines: ["/autoinventory"], note: "stow whatever is on the cursor" },
  { category: "Utility", name: "Loot All", color: 4, lines: ["/advloot", "/loot"], note: "open advanced loot, loot the corpse" },

  // ---- Travel / group ----
  { category: "Travel", name: "Rewind", color: 8, lines: ["/rewind"], note: "return to your last safe spot" },
  { category: "Travel", name: "Invite %t", color: 8, lines: ["/invite %t"], note: "invite your target to the group" },
  { category: "Travel", name: "Follow", color: 8, lines: ["/follow"], note: "auto-follow your target" },

  // ---- Social / RP ----
  { category: "Social", name: "Wave", color: 0, lines: ["/wave"] },
  { category: "Social", name: "GG", color: 0, lines: ["/say Good fight, %t!"], note: "uses the %t target token" },
];

/** distinct library categories in display order */
export const MACRO_CATEGORIES = Array.from(new Set(MACRO_LIBRARY.map((m) => m.category)));
