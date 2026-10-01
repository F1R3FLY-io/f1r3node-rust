--------------- MODULE MCIntroductionAuthorityRegistry ---------------
EXTENDS IntroductionAuthorityRegistry

CONSTANTS
    \* @type: Str;
    fallbackPayer,
    \* @type: Str;
    explicitPayer,
    \* @type: Str;
    noPayer

PayersDef == {fallbackPayer, explicitPayer}

=============================================================================
