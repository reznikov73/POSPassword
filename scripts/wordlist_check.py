#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""Проверка словарей для генератора фраз: уникальность и количество."""
import math

RU = """
солнце луна звезда небо облако туча гроза молния радуга дождь снег ветер буря
вьюга метель туман роса иней лёд вода огонь земля камень песок глина море
океан река ручей озеро пруд болото поле лес роща парк сад гора холм долина
ущелье пустыня степь тундра тайга остров берег пляж волна прибой прилив
отлив заря рассвет закат восход жара холод мороз оттепель погода климат
зима весна лето осень сезон время час минута секунда миг мгновение момент
период эпоха век год месяц неделя сутки утро день вечер ночь полдень полночь
волк лиса заяц медведь белка кабан лось олень тигр лев слон жираф зебра
обезьяна кошка собака лошадь корова бык овца коза свинья курица петух гусь
утка индюк кролик хомяк мышь крыса ёж крот бобр выдра норка соболь куница
горностай барсук енот панда коала кенгуру крокодил черепаха ящерица змея
кобра питон лягушка жаба рыба щука карась лещ сом окунь форель сельдь тунец
акула скат дельфин кит касатка тюлень морж пингвин чайка ворон ворона
сорока синица снегирь воробей скворец соловей жаворонок орёл сокол ястреб
сова филин голубь ласточка стриж аист цапля журавль лебедь куропатка
тетерев глухарь рябчик перепел фазан павлин попугай канарейка щегол чиж
клёст дятел кукушка стрекоза бабочка мотылёк пчела оса шмель муравей жук
кузнечик сверчок паук комар муха мошка светлячок улитка червяк
дерево куст трава цветок лист ветка ствол корень кора почка семя зерно
колос солома сено мох папоротник гриб ягода малина смородина крыжовник
клубника земляника черника брусника клюква рябина калина шиповник яблоко
груша слива вишня черешня абрикос персик айва алыча терн дуб клён ясень
липа берёза осина тополь ива верба ольха ель сосна кедр пихта лиственница
можжевельник тюльпан роза пион ирис гладиолус астра георгин нарцисс ландыш
фиалка ромашка василёк колокольчик незабудка гвоздика лилия орхидея
хризантема клевер люпин мак флокс резеда левкой
дом квартира комната кухня зал спальня прихожая балкон лоджия крыльцо крыша
стена пол потолок окно дверь форточка подоконник стол стул кресло диван
тахта кровать матрас подушка одеяло простыня наволочка полотенце зеркало
шкаф полка вешалка комод тумбочка лампа люстра торшер абажур занавеска
штора ковёр палас скатерть салфетка посуда тарелка миска чашка кружка
стакан бокал рюмка ложка вилка нож кастрюля сковорода чайник ковш дуршлаг
сито ножницы утюг пылесос метла совок тряпка щётка швабра ведро таз мыло
шампунь мочалка расчёска будильник часы календарь тетрадь блокнот записка
письмо конверт марка ручка карандаш кисть краски мел губка линейка ластик
точилка скрепка кнопка зажим клей бумага картон альбом книга журнал газета
словарь атлас карта глобус портфель ранец рюкзак сумка чемодан кошелёк
рубашка брюки штаны джинсы шорты юбка платье сарафан кофта свитер пуловер
джемпер толстовка куртка пальто плащ шуба дублёнка костюм пиджак жилет
комбинезон халат пижама майка футболка трусы носки гольфы колготки платок
шарф шапка ушанка кепка берет панама шляпа цилиндр перчатки рукавицы
варежки ремень пояс галстук бабочка ботинок туфля кроссовок кед валенок
лапоть тапок каблук подошва шнурок пряжка застёжка пуговица запонка брошка
кольцо серьга ожерелье бусы браслет цепочка кулон
хлеб батон булка плюшка сдоба кекс печенье пряник торт пирог кулебяка
ватрушка блин сырник пельмень вареник лапша каша гречка овсянка манка рис
пшено перловка мука тесто дрожжи соль сахар перец уксус масло сметана
творог сыр брынза молоко кефир ряженка простокваша йогурт сливки
мороженое яйцо омлет яичница котлета шницель отбивная гуляш плов рагу суп
борщ солянка рассольник уха бульон пюре винегрет салат закуска бутерброд
гамбургер картошка картофель морковь свёкла репа редька редис тыква кабачок
баклажан помидор огурец лук чеснок капуста горох фасоль бобы чечевица
зелень укроп петрушка сельдерей шпинат щавель компот кисель морс квас сок
чай кофе какао лимонад вино пиво коньяк шампанское наливка медовуха
город село деревня посёлок улица переулок проспект бульвар площадь
перекрёсток светофор дорога шоссе трасса тротуар бордюр мост тоннель вокзал
станция платформа поезд электричка трамвай троллейбус автобус маршрутка
такси метро машина автомобиль мотоцикл велосипед самокат скейтборд ролики
грузовик фургон прицеп трактор комбайн экскаватор бульдозер кран самолёт
вертолёт дирижабль шар ракета спутник корабль судно лодка шлюпка яхта катер
теплоход паром парус весло якорь пристань порт причал мачта палуба каюта
трюм капитан матрос штурман механик
человек люди ребёнок малыш парень девушка юноша мужчина женщина бабушка
дедушка мать отец папа мама сын дочь дочка брат сестра внук внучка
племянник тётя дядя муж жена сосед друг товарищ приятель знакомый гость
хозяин хозяйка работник сотрудник начальник директор врач доктор медсестра
хирург терапевт стоматолог учитель преподаватель профессор студент ученик
школьник инженер техник мастер рабочий строитель плотник столяр слесарь
сварщик электрик сантехник маляр каменщик водитель шофёр пилот машинист
кондуктор кассир продавец бухгалтер экономист юрист адвокат судья прокурор
следователь полицейский охранник солдат офицер генерал писатель поэт
художник скульптор композитор музыкант певец актёр режиссёр танцор
спортсмен тренер повар кондитер официант бармен парикмахер портной сапожник
часовщик ювелир фотограф журналист редактор переводчик библиотекарь учёный
исследователь агроном фермер пастух рыбак геолог шахтёр кузнец гончар
мысль идея разум память сон мечта желание надежда вера любовь радость
счастье горе печаль тоска скука страх ужас гнев ярость обида зависть
гордость стыд совесть честь слава похвала награда премия приз победа
поражение удача успех везение судьба доля чудо тайна загадка секрет истина
правда ложь обман слух молва весть новость известие сообщение разговор
беседа спор диалог молчание тишина покой отдых труд работа дело занятие
игра шутка смех веселье песня танец музыка мелодия ритм голос звук шум
грохот треск звон шёпот крик плач стон вздох слово речь язык буква слог
фраза предложение текст рассказ сказка предание легенда миф притча повесть
роман поэма стих рифма строка страница глава тема сюжет образ пример урок
наука знание опыт навык талант дар мастерство искусство ремесло техника
прогресс развитие рост перемена изменение
голова лицо глаз бровь ресница нос рот губа зуб ухо волос шея плечо рука
ладонь палец ноготь грудь спина живот поясница нога бедро колено голень
ступня пятка кожа кровь сердце мозг печень почка желудок мышца кость сустав
вена нерв дыхание пульс рост вес сила слабость здоровье болезнь лекарство
таблетка укол мазь бинт вата компресс градусник
инструмент молоток топор пила рубанок стамеска зубило напильник отвёртка
ключ плоскогубцы клещи дрель сверло гвоздь шуруп винт гайка шайба болт
скоба тиски наковальня провод кабель розетка выключатель лампочка батарея
радиатор котёл печь плита духовка холодильник морозилка телевизор монитор
компьютер ноутбук планшет телефон смартфон наушники колонка микрофон камера
фотоаппарат объектив вспышка штатив принтер сканер клавиатура коврик флешка
диск папка файл программа приложение экран курсор пароль логин сеть
интернет сайт ссылка почта вкладка браузер загрузка обновление батарейка
зарядка удлинитель фонарь прожектор фара бампер капот багажник колесо шина
руль педаль мотор двигатель бензин свеча аккумулятор антенна рация
космос планета комета метеорит галактика орбита
"""

EN = """
apple arrow acorn actor album alien alley amber anchor angel ankle answer
antler apron atlas atom aunt autumn avenue bacon badge bagel baker bamboo
banjo barn basket beach beacon beak beam bean bear beetle bell
berry bike bird birch bison blade blanket blaze blossom blue board
boat bobcat bonfire boulder branch brave bread bridge bright bronze brook
brown bubble buck buffalo bugle bull bunny bush butter button cabin cactus
camera candle canoe canvas canyon cardinal cargo carpet castle cattle cedar
cement cereal chalk charm cheese cheetah cherry chess chief chimney chipmunk
chisel cider cinema circle citron clamp clarity clover cluster coach coast
cobalt cobra coconut coffee comet compass copper coral cotton cougar course
cover coyote crane crater crayon creek cricket crimson crystal cupcake curve
cyclone dahlia daisy dance dawn deer delta denim desert diamond dime diner
dinner disco ditch diver dock dolphin domain donkey doodle double dove
dragon dream drift drum dryer duck dune dusk eagle earth easel echo eclipse
editor elder elk elm ember emerald empire engine envy equator ermine essay
evening event export extra fabric falcon family fawn feather fern fiddle
fig fir finch fire flame flannel flare fleet flint float flock flour flute
foam forest fork fossil fox frame frost fudge fuel gadget galaxy gallon
garden garlic gauge gazebo gecko gem genie giant gift ginger glacier glance
glass globe glider glow gold golf gondola gopher gorge granite
grape graphite gravel green grey grill grove guitar gulf gully habitat
halo hamster harbor harvest hawk hazel heather hedge helmet heron highway
hill hive hobby hockey honey hood horizon hornet horse hotel humble hunt
hurdle hut ice icicle idea igloo image import index inlet insect intent
invent iris island ivory ivy jade jaguar jasmine jeep jetty jewel jockey
jolly journal joy judge juggle juice jump junction jungle junior kayak
kelp kennel kernel kettle key kick kidney kilt kind king kitchen kite
kitten knee knife knight knob koala label lace ladder lake lagoon
lamp lantern lapel larch lark laser latitude laundry lava lavender lawn
layer leaf ledge legend lemon lentil leopard liberty lilac lily limb lime
linen lion lizard llama lobster locket lodge log loon lotus lumber lynx
lyric macaw magnet magnolia mahogany maize mall mango manor maple marble
marina market marsh mask mast meadow medal melody melon mentor mercy
meteor meter midge milk mill mimosa mineral mint mirror mist moat moccasin
mole monarch monsoon moped morning mortar mosaic moss motel mountain mouse
muffin mulberry mule museum music mustang mustard myrtle napkin
narrow nation nature navy nectar needle nest net newt nickel night nimble
noble nomad north notch novel nugget nutmeg oak oasis oat obelisk ocean
octave okra olive omega onion opal opera orange orbit orchard oregano
orchid osprey ostrich otter ounce outfit oval oven owl ox oyster paddle
palace palm panda pansy panther pantry papaya parade parcel park parrot
parsley parsnip pasta pasture patio pattern peach peanut pear pebble pecan
pelican pencil penny pepper perch petal pewter phantom pheasant phone
photo piano picnic pigeon pigment pilgrim pillow pilot pine piston pitcher
pivot plane planet plank plant plasma plaza plum plush pocket poem poet
point polar pollen pond poplar poppy porch port possum potato powder
prairie prize profile promenade propane prong puma pumpkin puzzle python
quail quarry quartz quiche quill quilt quiver rabbit raccoon radar radish
raft rail rainbow ranch rapid rascal raspberry raven ravine rebate redwood
reef refuge relay remedy rescue rhino rhythm ribbon ridge rifle
rinse ripple river rivet road roast robin rocket rodeo roller rooster rose
rotor round rubble ruby rudder runner runway rustic saffron sage salad
salmon salon salsa salt salvage sample sand sapphire sardine sash satin
sauce sauna savanna sawmill scale scarf scepter school scooter
scraper sea seal season seat sedan seed sequoia serene sesame shadow shale
shark shawl sheep shelter sherry shield ship shirt shovel shrimp shrine
sierra signal silk silver sketch skillet skyline slate sled sleet slope
smock snack snail snake snowflake socket sofa soft solar sonar sonnet
sorrel sound source south spade spark spatula spear spice spinach spiral
spruce spy square squid stable stadium stage stain stair stall
star steam steel steer stereo stilt stone stopper storm stove strand
straw stream street stucco studio suburb sugar summit sundial sunrise
sunset surf survey swan sweater swift sycamore syrup table tackle tadpole
tailor talent tango tapestry tavern tea teal tempo tender terrace
texture theater thaw thicket thimble thistle thorn thread throne thunder
ticket tide timber tinder tinsel tissue toad toaster toll topaz torch
tornado tortoise tower town trailer trail train tram tree
triangle tribune trout truce truffle trumpet tundra tunnel turbine turnip
turtle tusk tweed twilight twist umber umbrella unicorn uniform unison
upland urban urchin utensil valley vanilla vault velvet vendor verse
vessel viaduct vibe vicuna vial video violet vine vinyl viola viper visa
vision vista vitamin vocal volcano vortex voyage wafer wagon walnut walrus
wander warbler warden warm watch water wave weasel wedge well whale wharf
wheat wheel whisper whistle wicker widget wild wind window winter wire
wisdom wish wolf wonder woodland wool world worthy wren yacht yarn year
yarrow yellow yield yoga yogurt yolk young zebra zenith zephyr zeppelin
zinc zinnia zither zodiac zone
"""


def clean(words, name):
    seen, out, dups = set(), [], []
    for w in words.split():
        w = w.strip()
        if w in seen:
            dups.append(w)
            continue
        seen.add(w)
        out.append(w)
    print(f"{name}: {len(out)} уникальных; дубли: {len(dups)}")
    if dups:
        print("  дубли:", " ".join(dups))
    return out


ru = clean(RU, "RU")
en = clean(EN, "EN")
print(f"RU log2(N) = {math.log2(len(ru)):.2f} бит/слово")
print(f"EN log2(N) = {math.log2(len(en)):.2f} бит/слово")

with open("/home/z/my-project/scripts/words_ru.txt", "w", encoding="utf-8") as f:
    f.write("\n".join(ru))
with open("/home/z/my-project/scripts/words_en.txt", "w", encoding="utf-8") as f:
    f.write("\n".join(en))
print("Словари сохранены.")
